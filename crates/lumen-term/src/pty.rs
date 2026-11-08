//! A PTY with a reader thread, sized to the block it belongs to.

use crate::writer::InputWriter;
use prismattyc_emulator::{PtySession, pty_size_with_cell_pixels};
use std::io::Read;
#[cfg(all(unix, feature = "multiplexer-client"))]
use std::os::fd::{OwnedFd, RawFd};
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
    input: InputWriter,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    exited: bool,
    /// Whether the pseudo-console's start-up cursor query has been answered.
    #[cfg(windows)]
    answered_startup_query: bool,
}

/// The cursor-position query the Windows pseudo-console sends first.
const STARTUP_QUERY: &[u8] = b"\x1b[6n";
/// The terminal's answer to it: the cursor is at row 1, column 1.
#[cfg_attr(not(windows), allow(dead_code))]
const STARTUP_ANSWER: &[u8] = b"\x1b[1;1R";

/// Remove the pseudo-console's start-up cursor query from the front of `bytes`
/// the first time it is there. Returns whether it was, so the caller answers
/// it. The pseudo-console writes nothing else until it has an answer.
#[cfg_attr(not(windows), allow(dead_code))]
fn take_startup_query(answered: &mut bool, bytes: &mut Vec<u8>) -> bool {
    if *answered || !bytes.starts_with(STARTUP_QUERY) {
        return false;
    }
    *answered = true;
    bytes.drain(..STARTUP_QUERY.len());
    true
}

impl Pty {
    fn from_session(mut session: PtySession) -> Result<Self, SpawnError> {
        let mut reader = session
            .take_reader()
            .map_err(|e| SpawnError(e.to_string()))?;
        let input = session
            .take_input_writer()
            .map_err(|e| SpawnError(e.to_string()))?;
        let input = InputWriter::new(input);
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
            #[cfg(windows)]
            answered_startup_query: false,
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
        self.answer_startup_query(bytes)
    }

    /// Wait up to `wait` for the program's next output.
    pub fn poll_wait(&mut self, wait: std::time::Duration) -> Vec<u8> {
        let mut bytes = self.output.recv_timeout(wait).unwrap_or_default();
        while let Ok(chunk) = self.output.try_recv() {
            bytes.extend(chunk);
        }
        self.answer_startup_query(bytes)
    }

    /// On Windows, answer the pseudo-console's start-up cursor query, without
    /// which it passes on nothing the program writes.
    #[cfg(windows)]
    fn answer_startup_query(&mut self, mut bytes: Vec<u8>) -> Vec<u8> {
        if take_startup_query(&mut self.answered_startup_query, &mut bytes) {
            let _ = self.input.typed(STARTUP_ANSWER);
        }
        bytes
    }

    /// Elsewhere no such query comes.
    #[cfg(not(windows))]
    #[allow(clippy::unused_self)]
    fn answer_startup_query(&mut self, bytes: Vec<u8>) -> Vec<u8> {
        bytes
    }

    /// Type `bytes` to the program. The bytes are queued and written on another
    /// thread, so this never waits on the program.
    ///
    /// # Errors
    ///
    /// Returns `WouldBlock` if the program has left a long run of input unread,
    /// or `BrokenPipe` if the PTY can no longer be written.
    pub fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.input.typed(bytes)
    }

    /// Queue the terminal's own reply to the program. Never blocks. Returns
    /// whether it was queued; it is not when too many replies already wait.
    #[cfg(feature = "colour-replies")]
    pub fn send_reply(&self, bytes: Vec<u8>) -> bool {
        self.input.reply(bytes)
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
            // On Windows the reader's read does not return until the
            // pseudo-console is closed, which happens when the session is
            // dropped after this function. Joining here would wait for that
            // forever, so the thread is left to end when the console closes.
            #[cfg(not(windows))]
            let _ = handle.join();
            #[cfg(windows)]
            drop(handle);
        }
    }
}

#[cfg(test)]
mod startup_query_tests {
    use super::*;

    #[test]
    fn the_first_start_up_query_is_taken_and_only_the_first() {
        let mut answered = false;
        let mut bytes = b"\x1b[6nhello".to_vec();
        assert!(take_startup_query(&mut answered, &mut bytes));
        assert_eq!(bytes, b"hello");
        let mut again = b"\x1b[6nhello".to_vec();
        assert!(!take_startup_query(&mut answered, &mut again));
        assert_eq!(again, b"\x1b[6nhello");
    }

    #[test]
    fn output_without_the_query_at_its_front_is_left_alone() {
        let mut answered = false;
        let mut bytes = b"hi\x1b[6n".to_vec();
        assert!(!take_startup_query(&mut answered, &mut bytes));
        assert_eq!(bytes, b"hi\x1b[6n");
        assert!(!answered);
    }

    #[test]
    fn the_answer_is_a_cursor_position_report() {
        assert_eq!(STARTUP_ANSWER, b"\x1b[1;1R");
    }
}
