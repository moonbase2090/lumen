//! A program on a PTY, its bytes passing through the arm layer into a document.

use crate::document::{Block, Document};
use crate::layer::{Context, Layer};
use crate::pty::{Pty, SpawnError};
use std::time::{Duration, Instant};

/// The most columns a session takes, which also bounds the cells a block holds.
pub const MAX_COLUMNS: usize = 1000;
/// The most rows a session takes.
pub const MAX_ROWS: usize = 500;

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
    /// Run `program` with `layer` between its PTY and a document of `columns`
    /// by `rows`. The layer's container is open, as while a command runs.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    pub fn start<S: AsRef<std::ffi::OsStr>>(
        mut layer: Box<dyn Layer + Send>,
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
    ) -> Result<Self, SpawnError> {
        check_size(columns, rows)?;
        let pty = Pty::spawn(columns, rows, program, arguments)?;
        layer.open_container();
        Ok(Self {
            document: Document::new(columns, rows),
            layer,
            pty,
            clock: Instant::now(),
            // The program inherits this environment, so these are the layer's.
            tmux_or_screen: std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some(),
        })
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
