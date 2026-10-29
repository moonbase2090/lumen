//! A block of terminal cells with its own emulator and, optionally, its own PTY.

use prismattyc_emulator::{Emulator, PtySession, pty_size_with_cell_pixels};
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

/// Scrollback lines kept per block.
pub const SCROLLBACK: usize = 1000;
/// The cell size, in pixels, told to a program that asks. The window replaces
/// it with the real one.
const CELL_PX: (u32, u32) = (8, 16);

/// Why a program could not be started in a block.
#[derive(Debug)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not start the program: {}", self.0)
    }
}

impl std::error::Error for SpawnError {}

struct Running {
    session: PtySession,
    input: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

/// A block of `columns` by `rows` cells.
///
/// A block that runs a program owns that program's PTY, and the PTY is the
/// size of the block: resizing the block resizes the PTY, so the program sees
/// the size it really has.
pub struct GridBlock {
    emulator: Emulator,
    running: Option<Running>,
}

impl GridBlock {
    /// A block with no program, fed by [`feed`](Self::feed).
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            emulator: Emulator::new(columns, rows, SCROLLBACK),
            running: None,
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
        let size = pty_size_with_cell_pixels(columns, rows, CELL_PX.0, CELL_PX.1);
        let mut session =
            PtySession::spawn(program, arguments, size).map_err(|e| SpawnError(e.to_string()))?;
        let mut reader = session
            .take_reader()
            .map_err(|e| SpawnError(e.to_string()))?;
        let input = session
            .take_input_writer()
            .map_err(|e| SpawnError(e.to_string()))?;
        let (send, output) = channel();
        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || send.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut block = Self::new(columns, rows);
        block.running = Some(Running {
            session,
            input,
            output,
            reader: Some(handle),
        });
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
        let Some(running) = &self.running else {
            return Vec::new();
        };
        let mut bytes = Vec::new();
        while let Ok(chunk) = running.output.try_recv() {
            bytes.extend(chunk);
        }
        bytes
    }

    /// Wait up to `wait` for the program's next output.
    pub fn poll_wait(&mut self, wait: std::time::Duration) -> Vec<u8> {
        let Some(running) = &self.running else {
            return Vec::new();
        };
        let mut bytes = running.output.recv_timeout(wait).unwrap_or_default();
        while let Ok(chunk) = running.output.try_recv() {
            bytes.extend(chunk);
        }
        bytes
    }

    /// Type `bytes` to the program.
    ///
    /// # Errors
    ///
    /// Returns the write error, or an error if no program runs here.
    pub fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let Some(running) = &mut self.running else {
            return Err(std::io::Error::other("no program runs in this block"));
        };
        running.input.write_all(bytes)?;
        running.input.flush()
    }

    /// Resize the block, and the program's PTY with it.
    ///
    /// # Errors
    ///
    /// Returns the PTY error if the program could not be told.
    pub fn resize(&mut self, columns: usize, rows: usize) -> Result<(), SpawnError> {
        self.emulator.resize(columns, rows);
        if let Some(running) = &self.running {
            let size = pty_size_with_cell_pixels(columns, rows, CELL_PX.0, CELL_PX.1);
            running
                .session
                .resize(size)
                .map_err(|e| SpawnError(e.to_string()))?;
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

    /// Whether the program has exited.
    pub fn has_exited(&mut self) -> bool {
        match &mut self.running {
            Some(r) => r.session.try_wait().ok().flatten().is_some(),
            None => true,
        }
    }
}

impl Drop for GridBlock {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.session.kill_and_reap();
            if let Some(handle) = running.reader.take() {
                let _ = handle.join();
            }
        }
    }
}
