//! A program on a PTY, its bytes passing through the arm layer into a document.

use crate::document::{Block, Document};
use crate::layer::{Context, Layer};
use crate::pty::{Pty, SpawnError};
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
    document: Document,
    layer: Box<dyn Layer + Send>,
    pty: Pty,
    clock: Instant,
    tmux_or_screen: bool,
}

impl Session {
    fn from_pty(mut layer: Box<dyn Layer + Send>, columns: usize, rows: usize, pty: Pty) -> Self {
        layer.open_container();
        Self {
            document: Document::new(columns, rows),
            layer,
            pty,
            clock: Instant::now(),
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
        check_size(columns, rows)?;
        let pty = Pty::spawn(columns, rows, program, arguments)?;
        Ok(Self::from_pty(layer, columns, rows, pty))
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

    /// The document so far.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Take the events the document raised since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<crate::document::Event> {
        self.document.take_events()
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
        let ctx = self.context();
        self.document.receive(self.layer.as_mut(), bytes, ctx);
        bytes.len()
    }

    /// Take what the program has written, without waiting. Returns the number
    /// of bytes handled.
    pub fn pump(&mut self) -> usize {
        let bytes = self.pty.poll();
        self.take(&bytes)
    }

    /// Wait up to `wait` for the program's next output and take it.
    pub fn pump_wait(&mut self, wait: Duration) -> usize {
        let bytes = self.pty.poll_wait(wait);
        self.take(&bytes)
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
