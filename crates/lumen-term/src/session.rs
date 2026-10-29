//! A program on a PTY, its bytes passing through the arm layer into a document.

use crate::document::{Block, Document};
use crate::layer::{Context, Layer};
use crate::pty::{Pty, SpawnError};
use std::time::{Duration, Instant};

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

    /// The document so far.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
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

    /// The window is now `columns` by `rows`: every block and the program's
    /// PTY take the new size.
    ///
    /// # Errors
    ///
    /// Returns the PTY error if the program could not be told.
    pub fn resize(&mut self, columns: usize, rows: usize) -> Result<(), SpawnError> {
        self.document.resize(columns, rows);
        self.pty.resize(columns, rows)
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
