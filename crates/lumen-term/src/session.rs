//! A program on a PTY, its bytes passing through the arm layer into a document.

use crate::document::{Block, Document};
use crate::layer::{Context, Layer};
use crate::pty::{Pty, SpawnError};
#[cfg(feature = "colour-replies")]
use crate::raster::Theme;
#[cfg(feature = "colour-replies")]
use crate::reply::Replies;
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
    #[cfg(feature = "colour-replies")]
    replies: Replies,
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
            #[cfg(feature = "colour-replies")]
            replies: Replies::new(crate::design::theme()),
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

    fn take(&mut self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        self.marks.feed(bytes);
        let ctx = self.context();
        self.document.receive(self.layer.as_mut(), bytes, ctx);
        let queries = self.document.take_colour_queries();
        #[cfg(feature = "colour-replies")]
        if !queries.is_empty() {
            let reply = self.replies.answer(&queries, ctx.now_ms);
            if !reply.is_empty() {
                self.pty.send_reply(reply);
            }
        }
        #[cfg(not(feature = "colour-replies"))]
        drop(queries);
        let reports = self.document.take_cursor_reports();
        #[cfg(feature = "colour-replies")]
        if !reports.is_empty() {
            let reply = self.replies.answer_cursor(&reports, ctx.now_ms);
            if !reply.is_empty() {
                self.pty.send_reply(reply);
            }
        }
        #[cfg(not(feature = "colour-replies"))]
        drop(reports);
        bytes.len()
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
        self.document.finish(self.layer.as_mut());
    }

    /// A new prompt cycle begins.
    pub fn begin_prompt_cycle(&mut self) {
        self.document.begin_prompt_cycle(self.layer.as_mut());
    }
}
