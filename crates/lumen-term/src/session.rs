//! A program on a PTY, its bytes passing through the arm layer into a document.

use crate::document::{Block, Document};
use crate::layer::{Context, Layer};
use crate::pty::{Pty, SpawnError};
#[cfg(feature = "colour-replies")]
use crate::raster::Theme;
use crate::reply::{CursorQueryEnds, MAX_PER_READ, MAX_SPLITS_PER_READ, Replies};
use crate::shellint::ShellOptions;
#[cfg(all(unix, feature = "multiplexer-client"))]
use std::os::fd::OwnedFd;
#[cfg(all(unix, feature = "multiplexer-client"))]
use std::path::Path;
use std::time::{Duration, Instant};

/// The most columns a session takes, which also bounds the cells a block holds.
pub const MAX_COLUMNS: usize = 1000;
/// The most rows a session takes.
pub const MAX_ROWS: usize = 500;

/// The descriptor on which a spawned Lumen-aware multiplexer client receives
/// its scoped key. The key bytes never enter argv, the environment or the PTY.
#[cfg(all(unix, feature = "multiplexer-client"))]
pub const MULTIPLEXER_KEY_FD: i32 = 3;

/// The executable selected by a multiplexer profile or Attach action.
///
/// Construction resolves the configured path before a key fd can be supplied.
/// The resolved path is the exact program the terminal spawns.
#[cfg(all(unix, feature = "multiplexer-client"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiplexerClientProgram(String);

#[cfg(all(unix, feature = "multiplexer-client"))]
impl MultiplexerClientProgram {
    /// Resolve and check the client executable configured by a profile or
    /// Attach action.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved, is not a regular file
    /// or cannot be represented for the PTY spawn API.
    pub fn from_profile_path(path: impl AsRef<Path>) -> Result<Self, SpawnError> {
        let executable = std::fs::canonicalize(path)
            .map_err(|e| SpawnError(format!("could not resolve multiplexer client: {e}")))?;
        if !executable.is_file() {
            return Err(SpawnError(
                "multiplexer client is not a regular file".to_owned(),
            ));
        }
        let Some(executable) = executable.to_str() else {
            return Err(SpawnError(
                "multiplexer client path is not valid Unicode".to_owned(),
            ));
        };
        Ok(Self(executable.to_owned()))
    }

    fn executable(&self) -> &str {
        &self.0
    }
}

/// The capability a sealed frame type is sealed under, for the layer.
#[cfg(target_os = "linux")]
fn sealed_capability(kind: u8) -> u8 {
    lumen_sealed::types::capability_of(kind).unwrap_or(0)
}

fn check_size(columns: usize, rows: usize) -> Result<(), SpawnError> {
    if (1..=MAX_COLUMNS).contains(&columns) && (1..=MAX_ROWS).contains(&rows) {
        Ok(())
    } else {
        Err(SpawnError(format!(
            "a size of {columns} by {rows} is outside 1..={MAX_COLUMNS} by 1..={MAX_ROWS}"
        )))
    }
}

/// One program, one PTY, one layer and the document they make.
///
/// The PTY is the size of the document's blocks, so the program sees the size
/// of the block its output lands in. Every byte it writes goes through the
/// layer before the emulator sees it.
pub struct Session {
    /// The holder and what goes with it, for a sealed shell. Declared before
    /// the PTY so the holder ends before the shell's PTY closes.
    #[cfg(target_os = "linux")]
    sealing: Option<crate::seal::Sealing>,
    document: Document,
    layer: Box<dyn Layer + Send>,
    pty: Pty,
    /// The shell's hook files, which go when the session does.
    shell_files: Option<crate::shellint::Scratch>,
    /// The prompt marks the program has written.
    marks: crate::marks::MarkScanner,
    clock: Instant,
    tmux_or_screen: bool,
    replies: Replies,
    /// Where the program has got to in writing a cursor-position query.
    cursor_queries: CursorQueryEnds,
    /// The rest of a read, not given to the layer yet: the document paused at
    /// a sealed frame (see [`resume`](Self::resume)).
    carry: Vec<u8>,
    /// What the read being taken may still do.
    read: ReadState,
}

/// The state of one read while it is taken, which can span calls when the
/// document pauses at a sealed frame.
#[derive(Debug, Clone, Copy, Default)]
struct ReadState {
    ctx: Context,
    /// What the whole read may still have answered.
    left: usize,
    /// How many pieces it was split into at cursor queries.
    splits: usize,
}

impl Session {
    fn from_pty(mut layer: Box<dyn Layer + Send>, columns: usize, rows: usize, pty: Pty) -> Self {
        layer.open_container();
        Self {
            #[cfg(target_os = "linux")]
            sealing: None,
            document: Document::new(columns, rows),
            layer,
            pty,
            shell_files: None,
            marks: crate::marks::MarkScanner::new(),
            clock: Instant::now(),
            replies: Replies::new(crate::design::theme()),
            cursor_queries: CursorQueryEnds::default(),
            carry: Vec::new(),
            read: ReadState::default(),
            // The program inherits this environment, so these are the layer's.
            tmux_or_screen: std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some(),
        }
    }

    /// Run `program` with `layer` between its PTY and a document of `columns`
    /// by `rows`. The layer's container is open, as while a command runs.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    pub fn start<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
    ) -> Result<Self, SpawnError> {
        Self::start_with_shell(
            layer,
            columns,
            rows,
            program,
            arguments,
            ShellOptions::default(),
        )
    }

    /// [`start`](Self::start) with the shell's options given: whether a shell
    /// started directly gets the prompt-mark hooks (see [`shellint`](crate::shellint)).
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    pub fn start_with_shell<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        shell: ShellOptions,
    ) -> Result<Self, SpawnError> {
        #[cfg(target_os = "linux")]
        return Self::start_with_seal_and_shell(
            layer,
            columns,
            rows,
            program,
            arguments,
            &crate::seal::Config::default(),
            shell,
        );
        #[cfg(not(target_os = "linux"))]
        {
            Self::start_in_environment(
                layer,
                columns,
                rows,
                program,
                arguments,
                shell,
                &std::collections::BTreeMap::new(),
            )
        }
    }

    /// [`start_with_shell`](Self::start_with_shell) with `environment` added to
    /// the program's own, and no sealed layer. This is how a shell is started
    /// in a home directory of its own.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    pub fn start_in_environment<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        shell: ShellOptions,
        environment: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, SpawnError> {
        check_size(columns, rows)?;
        Self::start_unsealed(layer, columns, rows, program, arguments, shell, environment)
    }

    /// Start `program` on a PTY with no sealed layer. A shell started directly
    /// gets the hooks `shell` allows.
    fn start_unsealed<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        shell: ShellOptions,
        environment: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, SpawnError> {
        if !arguments.is_empty() {
            let pty = if environment.is_empty() {
                Pty::spawn(columns, rows, program, arguments)?
            } else {
                Pty::spawn_with_environment(columns, rows, program, arguments, environment)?
            };
            return Ok(Self::from_pty(layer, columns, rows, pty));
        }
        // What the shell is given counts as its environment for the plan too.
        let var = |name: &str| {
            environment
                .get(name)
                .cloned()
                .or_else(|| std::env::var(name).ok())
        };
        let launch = crate::shellint::plan(program, &[], shell, &var);
        let mut added = environment.clone();
        added.extend(launch.environment.clone());
        let pty =
            Pty::spawn_with_environment(columns, rows, &launch.program, &launch.arguments, &added)?;
        let mut session = Self::from_pty(layer, columns, rows, pty);
        session.shell_files = launch.scratch;
        Ok(session)
    }

    /// [`start`](Self::start) with the sealed layer's settings given. A listed
    /// shell started directly gets a sealed layer if the sealing conditions
    /// hold; anything else runs as before.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    #[cfg(target_os = "linux")]
    pub fn start_with_seal<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        config: &crate::seal::Config,
    ) -> Result<Self, SpawnError> {
        Self::start_with_seal_and_shell(
            layer,
            columns,
            rows,
            program,
            arguments,
            config,
            ShellOptions::default(),
        )
    }

    /// [`start_with_seal`](Self::start_with_seal) with the shell's options
    /// given. A shell that gets a sealed layer already has its own hooks, so
    /// only a shell that does not is given the prompt marks.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    #[cfg(target_os = "linux")]
    pub fn start_with_seal_and_shell<S: AsRef<std::ffi::OsStr>>(
        mut layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        config: &crate::seal::Config,
        shell: ShellOptions,
    ) -> Result<Self, SpawnError> {
        check_size(columns, rows)?;
        // A listed shell started directly (no arguments, so no `-c`) is run by
        // its resolved path; anything else runs as before.
        let resolved = if arguments.is_empty() {
            crate::seal::resolve(config, program)
        } else {
            None
        };
        let Some(resolved) = resolved else {
            return Self::start_unsealed(
                layer,
                columns,
                rows,
                program,
                arguments,
                shell,
                &config.environment,
            );
        };
        let started = crate::seal::start(columns, rows, &resolved, config)?;
        if let Some(receiver) = started.receiver {
            layer.attach_sealed(receiver, sealed_capability);
        }
        let mut session = Self::from_pty(layer, columns, rows, started.pty);
        session.sealing = Some(started.sealing);
        Ok(session)
    }

    /// Whether the shell has a sealed layer, and if not, why. Not applicable
    /// for a program that is not a listed shell started directly.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn seal_status(&self) -> crate::seal::Status {
        self.sealing
            .as_ref()
            .map_or(crate::seal::Status::NotApplicable, |s| s.status().clone())
    }

    /// Whether the layer has sealing keys now: it was sealed, it is not pinned
    /// to grid mode, nothing has made it grid-only and its holder runs. Always
    /// false where there is no sealed layer (every system but Linux).
    pub fn has_sealed_keys(&mut self) -> bool {
        if self
            .document
            .notices()
            .contains(&crate::layer::Notice::Pinned)
        {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            self.sealing
                .as_mut()
                .is_some_and(crate::seal::Sealing::has_keys)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Why a shell layer has no sealing keys, in the terminal's own words, or
    /// `None` while it has them or when there is no layer to seal (a program
    /// that is not a listed shell, or a system without sealing). Nothing in it
    /// comes from the program.
    #[must_use]
    pub fn seal_off_reason(&self) -> Option<String> {
        if self
            .document
            .notices()
            .contains(&crate::layer::Notice::Pinned)
        {
            return Some("too many sealed frames were refused".to_owned());
        }
        #[cfg(target_os = "linux")]
        if let crate::seal::Status::GridOnly(why) = self.seal_status() {
            return Some(why);
        }
        None
    }

    /// Run the sealing checks required at every Enter the terminal forwards
    /// for the user. A layer that fails them loses its keys for good. Nothing
    /// to do on a layer without keys.
    pub fn recheck_seal(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(sealing) = self.sealing.as_mut() {
            sealing.recheck();
        }
    }

    /// The holder's process id, if the layer is sealed.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn holder_pid(&self) -> Option<u32> {
        self.sealing
            .as_ref()
            .and_then(crate::seal::Sealing::holder_pid)
    }

    /// Spawn the checked multiplexer client with its scoped key source mapped
    /// to [`MULTIPLEXER_KEY_FD`] in that child only.
    ///
    /// `key_source` must already be close-on-exec. It is consumed whether the
    /// spawn succeeds or fails. The terminal never maps it into an existing
    /// process, an ordinary session or another concurrent child.
    ///
    /// # Errors
    ///
    /// Returns why the size, key source, PTY or client spawn was refused.
    #[cfg(all(unix, feature = "multiplexer-client"))]
    pub fn start_multiplexer_client<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        client: &MultiplexerClientProgram,
        arguments: &[S],
        key_source: OwnedFd,
    ) -> Result<Self, SpawnError> {
        check_size(columns, rows)?;
        let pty = Pty::spawn_multiplexer_client(
            columns,
            rows,
            client.executable(),
            arguments,
            key_source,
            MULTIPLEXER_KEY_FD,
        )?;
        Ok(Self::from_pty(layer, columns, rows, pty))
    }

    /// Give the session's layer the receiver for frames sealed to the terminal.
    /// Only a session whose layer has sealing keys calls this; without it every
    /// sealed-range frame stays escaped text.
    pub fn attach_sealed(&mut self, receiver: lumen_sealed::Receiver, cap_of: fn(u8) -> u8) {
        self.layer.attach_sealed(receiver, cap_of);
    }

    /// The prompt marks the shell has written (OSC 133 and OSC 7), oldest
    /// first. They change nothing in the document.
    #[must_use]
    pub fn marks(&self) -> Vec<crate::marks::Mark> {
        self.marks.marks()
    }

    /// The document so far.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Take the window title the program set since the last call.
    pub fn take_title(&mut self) -> Option<String> {
        self.document.take_title()
    }

    /// Whether the program rang the bell since the last call.
    pub fn take_bell(&mut self) -> bool {
        self.document.take_bell()
    }

    /// Take the events the document raised since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<crate::document::Event> {
        self.document.take_events()
    }

    /// Answer colour queries from `theme` from now on.
    #[cfg(feature = "colour-replies")]
    pub fn set_theme(&mut self, theme: Theme) {
        self.replies.set_theme(theme);
    }

    /// The local `images` setting: `false` always shows the alt text.
    pub fn set_images_setting(&mut self, on: bool) {
        self.document.set_images_setting(on);
    }

    /// The key modes the program has set: application cursor keys and keypad.
    /// Each grid block records what it changed; they are folded in order, so a
    /// mode survives a rich block between two grid blocks.
    #[must_use]
    pub fn key_modes(&self) -> crate::modes::KeyModes {
        crate::modes::key_modes_for(&self.document)
    }

    /// Milliseconds since the session started: the clock the notifier uses.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn context(&self) -> Context {
        let alt_screen =
            matches!(self.document.blocks().last(), Some(Block::Grid(g)) if g.alt_active());
        Context {
            alt_screen,
            tmux_or_screen: self.tmux_or_screen,
            now_ms: u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }

    /// Give the document `piece` and answer what it asked, colour queries
    /// first (they came before the piece's closing cursor query, if it has one).
    /// `left` is what the whole read may still have answered.
    fn give(&mut self, piece: &[u8], ctx: Context, left: &mut usize) {
        self.document.receive(self.layer.as_mut(), piece, ctx);
        self.answer(ctx, left);
    }

    /// Answer what the document applied since the last answer asked: colour
    /// queries first, then cursor queries.
    fn answer(&mut self, ctx: Context, left: &mut usize) {
        let queries = self.document.take_colour_queries();
        #[cfg(feature = "colour-replies")]
        if !queries.is_empty() {
            let reply = self.replies.answer_within(&queries, ctx.now_ms, left);
            if !reply.is_empty() {
                self.pty.send_reply(reply);
            }
        }
        #[cfg(not(feature = "colour-replies"))]
        drop(queries);
        let reports: Vec<(u32, u32)> = self
            .document
            .blocks()
            .iter()
            .filter_map(|b| match b {
                Block::Grid(g) => Some(g.take_cursor_reports()),
                Block::Rich(_) => None,
            })
            .flatten()
            .collect();
        if !reports.is_empty() {
            let reply = self
                .replies
                .answer_cursor_within(&reports, ctx.now_ms, left);
            if !reply.is_empty() {
                self.pty.send_reply(reply);
            }
        }
    }

    fn take(&mut self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        self.marks.feed(bytes);
        self.read = ReadState {
            ctx: self.context(),
            left: MAX_PER_READ,
            splits: 0,
        };
        self.carry.extend_from_slice(bytes);
        self.give_carry();
        bytes.len()
    }

    /// Give the layer what is carried of the read. A program may ask a colour
    /// question and a cursor question in one read and expects the answers in
    /// that order. Colour queries are taken out of the stream before the
    /// emulator sees it and cursor queries in it, so the read is given in
    /// pieces that each end at a cursor query, and each piece is answered
    /// before the next is given. If the document pauses at a sealed frame, the
    /// rest waits in `carry` for [`resume`](Self::resume).
    fn give_carry(&mut self) {
        let bytes = std::mem::take(&mut self.carry);
        let mut read = self.read;
        let mut start = 0;
        for (at, byte) in bytes.iter().enumerate() {
            if self.cursor_queries.step(*byte) && read.splits < MAX_SPLITS_PER_READ {
                read.splits += 1;
                self.give(&bytes[start..=at], read.ctx, &mut read.left);
                start = at + 1;
                if self.document.paused() {
                    break;
                }
            }
        }
        if self.document.paused() {
            self.carry = bytes[start..].to_vec();
        } else if start < bytes.len() {
            self.give(&bytes[start..], read.ctx, &mut read.left);
        }
        self.read = read;
    }

    /// Stop after each sealed frame the layer gives, so the caller can take
    /// the frame's event and act on it with the document as it was when the
    /// frame came, before the output after it in the same read. The caller
    /// then calls [`resume`](Self::resume) until it returns false. Off (the
    /// default), a read is applied whole.
    pub fn pause_at_sealed(&mut self, on: bool) {
        self.document.pause_at_sealed(on);
    }

    /// Go on with a read the document paused at a sealed frame: apply what
    /// followed the frame, up to the next one. Returns false when nothing of
    /// the read was left.
    pub fn resume(&mut self) -> bool {
        if self.document.paused() {
            let mut read = self.read;
            self.document.apply_deferred();
            self.answer(read.ctx, &mut read.left);
            self.read = read;
        } else if !self.carry.is_empty() {
            self.give_carry();
        } else {
            return false;
        }
        self.document.poll_images();
        true
    }

    /// Take what the program has written, without waiting. Returns the number
    /// of bytes handled.
    pub fn pump(&mut self) -> usize {
        let bytes = self.pty.poll();
        let n = self.take(&bytes);
        self.document.poll_images();
        n
    }

    /// Wait up to `wait` for the program's next output and take it.
    pub fn pump_wait(&mut self, wait: Duration) -> usize {
        let bytes = self.pty.poll_wait(wait);
        let n = self.take(&bytes);
        self.document.poll_images();
        n
    }

    /// Type `bytes` to the program.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.pty.send(bytes)
    }

    /// The window is now `columns` by `rows`: the program's PTY and every block
    /// take the new size. The PTY is told first, so if that fails (or the size
    /// is outside the limits) the document keeps its old size and the two stay
    /// in step.
    ///
    /// # Errors
    ///
    /// Returns why the size was refused or the program could not be told.
    pub fn resize(&mut self, columns: usize, rows: usize) -> Result<(), SpawnError> {
        check_size(columns, rows)?;
        self.pty.resize(columns, rows)?;
        self.document.resize(columns, rows);
        Ok(())
    }

    /// The process group in the foreground of the program's terminal, if the
    /// system can say (see [`Pty::foreground_group`]).
    #[must_use]
    pub fn foreground_group(&self) -> Option<i32> {
        self.pty.foreground_group()
    }

    /// The tty's view of who reads it: see [`Pty::tty_state`].
    #[must_use]
    pub fn tty_state(&self) -> crate::pty::TtyState {
        self.pty.tty_state()
    }

    /// Whether the program has exited.
    pub fn has_exited(&mut self) -> bool {
        self.pty.has_exited()
    }

    /// The stream ended: release what the layer holds back.
    pub fn finish(&mut self) {
        while self.resume() {}
        self.document.finish(self.layer.as_mut());
    }

    /// A new prompt cycle begins.
    pub fn begin_prompt_cycle(&mut self) {
        self.document.begin_prompt_cycle(self.layer.as_mut());
    }
}
