//! A PTY with a reader thread, sized to the block it belongs to.

use prismattyc_emulator::{PtySession, pty_size_with_cell_pixels};
use std::io::{Read, Write};
#[cfg(all(unix, feature = "multiplexer-client"))]
use std::os::fd::{OwnedFd, RawFd};
use std::sync::mpsc::{Receiver, channel};
#[cfg(feature = "colour-replies")]
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The cell size, in pixels, told to a program that asks. The window replaces
/// it with the real one.
const CELL_PX: (u32, u32) = (8, 16);

/// Replies that can wait to be written; more are dropped.
#[cfg(feature = "colour-replies")]
const REPLY_QUEUE: usize = 16;

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
    input: Arc<Mutex<Box<dyn Write + Send>>>,
    #[cfg(feature = "colour-replies")]
    replies: SyncSender<Vec<u8>>,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    exited: bool,
}

impl Pty {
    fn from_session(mut session: PtySession) -> Result<Self, SpawnError> {
        let mut reader = session
            .take_reader()
            .map_err(|e| SpawnError(e.to_string()))?;
        let input = session
            .take_input_writer()
            .map_err(|e| SpawnError(e.to_string()))?;
        let input = Arc::new(Mutex::new(input));
        #[cfg(feature = "colour-replies")]
        let replies = {
            // Replies are written on their own thread through a short queue, so a
            // program that never reads its input cannot stall the terminal by asking
            // for more replies than its PTY will take. A full queue drops the reply.
            let (replies, queued) = sync_channel::<Vec<u8>>(REPLY_QUEUE);
            let writer = Arc::clone(&input);
            std::thread::spawn(move || {
                while let Ok(bytes) = queued.recv() {
                    let Ok(mut w) = writer.lock() else { break };
                    if w.write_all(&bytes).and_then(|()| w.flush()).is_err() {
                        break;
                    }
                }
            });
            replies
        };
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
            #[cfg(feature = "colour-replies")]
            replies,
            output,
            reader: Some(handle),
            exited: false,
        })
    }

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
        let session =
            PtySession::spawn(program, arguments, size).map_err(|e| SpawnError(e.to_string()))?;
        Self::from_session(session)
    }

    /// Run a configured multiplexer client and map its key source into only
    /// that child at `key_target`.
    #[cfg(all(unix, feature = "multiplexer-client"))]
    pub(crate) fn spawn_multiplexer_client<S: AsRef<std::ffi::OsStr>>(
        columns: usize,
        rows: usize,
        program: &str,
        arguments: &[S],
        key_source: OwnedFd,
        key_target: RawFd,
    ) -> Result<Self, SpawnError> {
        let size = pty_size_with_cell_pixels(columns, rows, CELL_PX.0, CELL_PX.1);
        let session = PtySession::spawn_config_with_fds(
            program,
            arguments,
            None,
            &std::collections::BTreeMap::new(),
            size,
            vec![(key_source, key_target)],
        )
        .map_err(|e| SpawnError(e.to_string()))?;
        Self::from_session(session)
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
        let mut input = self
            .input
            .lock()
            .map_err(|_| std::io::Error::other("the PTY writer is poisoned"))?;
        input.write_all(bytes)?;
        input.flush()
    }

    #[cfg(feature = "colour-replies")]
    /// Queue the terminal's own reply to the program. Never blocks. Returns
    /// whether it was queued; it is not when the queue is full or the writer has
    /// stopped.
    pub fn send_reply(&self, bytes: Vec<u8>) -> bool {
        !matches!(
            self.replies.try_send(bytes),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_))
        )
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
