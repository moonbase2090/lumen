//! A block of terminal cells with its own emulator and, optionally, its own PTY.

use crate::pty::{Pty, SpawnError};
use prismattyc_emulator::Emulator;

/// Scrollback lines kept per block.
pub const SCROLLBACK: usize = 1000;

/// A block of `columns` by `rows` cells.
///
/// A block that runs a program owns that program's PTY, and the PTY is the
/// size of the block: resizing the block resizes the PTY, so the program sees
/// the size it really has. Bytes from that PTY reach the emulator only when the
/// owner [`feed`](Self::feed)s them, so an arm layer can sit in between; the
/// [`Session`](crate::Session) does that.
pub struct GridBlock {
    emulator: Emulator,
    pty: Option<Pty>,
}

impl GridBlock {
    /// A block with no program, fed by [`feed`](Self::feed).
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            emulator: Emulator::new(columns, rows, SCROLLBACK),
            pty: None,
        }
    }

    /// A block that runs `program` on a PTY of the block's size.
    ///
    /// # Errors
    ///
    /// Returns why the PTY or the program could not be started.
    pub fn spawn<S: AsRef<std::ffi::OsStr>>(
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
    ) -> Result<Self, SpawnError> {
        let mut block = Self::new(columns, rows);
        block.pty = Some(Pty::spawn(columns, rows, program, arguments)?);
        Ok(block)
    }

    /// The size in columns and rows.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        let screen = self.emulator.screen();
        (screen.columns(), screen.rows())
    }

    /// The screen, for a renderer.
    #[must_use]
    pub fn screen(&self) -> &prismattyc_core::Screen {
        self.emulator.screen()
    }

    /// Give the emulator bytes, as if the program had written them.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.emulator.feed(bytes);
    }

    /// The bytes the program has written since the last call, without waiting.
    pub fn poll(&mut self) -> Vec<u8> {
        self.pty.as_mut().map(Pty::poll).unwrap_or_default()
    }

    /// Wait up to `wait` for the program's next output.
    pub fn poll_wait(&mut self, wait: std::time::Duration) -> Vec<u8> {
        self.pty
            .as_mut()
            .map(|p| p.poll_wait(wait))
            .unwrap_or_default()
    }

    /// Type `bytes` to the program.
    ///
    /// # Errors
    ///
    /// Returns the write error, or an error if no program runs here.
    pub fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match &mut self.pty {
            Some(pty) => pty.send(bytes),
            None => Err(std::io::Error::other("no program runs in this block")),
        }
    }

    /// Resize the block, and the program's PTY with it.
    ///
    /// # Errors
    ///
    /// Returns the PTY error if the program could not be told.
    pub fn resize(&mut self, columns: usize, rows: usize) -> Result<(), SpawnError> {
        self.emulator.resize(columns, rows);
        if let Some(pty) = &self.pty {
            pty.resize(columns, rows)?;
        }
        Ok(())
    }

    /// The text of each row, with trailing blanks removed.
    #[must_use]
    pub fn text_rows(&self) -> Vec<String> {
        let screen = self.emulator.screen();
        (0..screen.rows())
            .map(|r| {
                let line: String = screen
                    .row(r)
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !c.wide_cont)
                    .map(|c| c.character)
                    .collect();
                line.trim_end().to_owned()
            })
            .collect()
    }

    /// Whether the alternate screen is active.
    #[must_use]
    pub fn alt_active(&self) -> bool {
        self.emulator.screen().alt_active()
    }

    /// Whether the program has exited.
    pub fn has_exited(&mut self) -> bool {
        match &mut self.pty {
            Some(pty) => pty.has_exited(),
            None => true,
        }
    }
}
