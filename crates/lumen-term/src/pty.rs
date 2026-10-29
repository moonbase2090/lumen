//! A PTY with a reader thread, sized to the block it belongs to.

use prismattyc_emulator::{PtySession, pty_size_with_cell_pixels};
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

/// The cell size, in pixels, told to a program that asks. The window replaces
/// it with the real one.
const CELL_PX: (u32, u32) = (8, 16);

/// Why a program could not be started or told its size.
#[derive(Debug)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not start the program: {}", self.0)
    }
}

impl std::error::Error for SpawnError {}

/// A program on a PTY. Its output is read on a thread and collected for the
/// owner to take when it chooses.
pub struct Pty {
    session: PtySession,
    input: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    exited: bool,
}

impl Pty {
    /// Run `program` on a PTY of `columns` by `rows`.
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
        Ok(Self {
            session,
            input,
            output,
            reader: Some(handle),
            exited: false,
        })
    }

    /// The bytes the program has written since the last call, without waiting.
    pub fn poll(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Ok(chunk) = self.output.try_recv() {
            bytes.extend(chunk);
        }
        bytes
    }

    /// Wait up to `wait` for the program's next output.
    pub fn poll_wait(&mut self, wait: std::time::Duration) -> Vec<u8> {
        let mut bytes = self.output.recv_timeout(wait).unwrap_or_default();
        while let Ok(chunk) = self.output.try_recv() {
            bytes.extend(chunk);
        }
        bytes
    }

    /// Type `bytes` to the program.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.input.write_all(bytes)?;
        self.input.flush()
    }

    /// Tell the program its size is now `columns` by `rows`.
    ///
    /// # Errors
    ///
    /// Returns the PTY error if the program could not be told.
    pub fn resize(&self, columns: usize, rows: usize) -> Result<(), SpawnError> {
        let size = pty_size_with_cell_pixels(columns, rows, CELL_PX.0, CELL_PX.1);
        self.session
            .resize(size)
            .map_err(|e| SpawnError(e.to_string()))
    }

    /// Whether the program has exited. Once it has, this stays true: the
    /// status is read once, and asking again would find nothing to reap.
    pub fn has_exited(&mut self) -> bool {
        if !self.exited {
            self.exited = self.session.try_wait().ok().flatten().is_some();
        }
        self.exited
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.session.kill_and_reap();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}
