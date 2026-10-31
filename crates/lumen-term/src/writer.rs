//! The only writer to a program's PTY input.
//!
//! A PTY holds a few kilobytes of input. A program that never reads it stops
//! every write that follows, so no thread the interface runs on may write to the
//! PTY itself. Typed input and the terminal's own replies are queued here, and
//! one thread, which owns the writer and takes no lock anyone else waits on,
//! writes them in order. Queueing never blocks:
//!
//! - typed input is refused with [`std::io::ErrorKind::WouldBlock`] only when the
//!   program has left a long run of it unread, and is never dropped to make room
//!   for a reply;
//! - a reply is dropped when [`REPLY_QUEUE`] replies are already waiting, or when
//!   the queue is full.

use std::io::{self, Write};
#[cfg(feature = "colour-replies")]
use std::sync::Arc;
#[cfg(feature = "colour-replies")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

/// Typed writes that can wait. Each is one key or one paste.
pub(crate) const TYPED_QUEUE: usize = 256;

/// Replies that can wait; more are dropped.
#[cfg(feature = "colour-replies")]
pub(crate) const REPLY_QUEUE: usize = 16;

/// The queue room kept for replies: none when they are off.
#[cfg(feature = "colour-replies")]
const REPLY_ROOM: usize = REPLY_QUEUE;
#[cfg(not(feature = "colour-replies"))]
const REPLY_ROOM: usize = 0;

enum Message {
    Typed(Vec<u8>),
    #[cfg(feature = "colour-replies")]
    Reply(Vec<u8>),
}

/// Queues bytes for a PTY and writes them on a thread of its own.
pub(crate) struct InputWriter {
    queue: SyncSender<Message>,
    #[cfg(feature = "colour-replies")]
    replies_waiting: Arc<AtomicUsize>,
}

impl InputWriter {
    /// Take `writer` and start the thread that writes to it. The thread ends
    /// when the `InputWriter` is dropped or a write fails.
    pub(crate) fn new(mut writer: Box<dyn Write + Send>) -> Self {
        // The queue holds the most of both kinds that may wait, so a full run of
        // replies cannot take the room typed input is promised.
        let (queue, waiting) = sync_channel::<Message>(TYPED_QUEUE + REPLY_ROOM);
        #[cfg(feature = "colour-replies")]
        let replies_waiting = Arc::new(AtomicUsize::new(0));
        #[cfg(feature = "colour-replies")]
        let counter = Arc::clone(&replies_waiting);
        std::thread::spawn(move || {
            while let Ok(message) = waiting.recv() {
                #[cfg(feature = "colour-replies")]
                let (bytes, reply) = match message {
                    Message::Typed(bytes) => (bytes, false),
                    Message::Reply(bytes) => (bytes, true),
                };
                #[cfg(not(feature = "colour-replies"))]
                let Message::Typed(bytes) = message;
                let written = writer.write_all(&bytes).and_then(|()| writer.flush());
                #[cfg(feature = "colour-replies")]
                if reply {
                    counter.fetch_sub(1, Ordering::SeqCst);
                }
                if written.is_err() {
                    break;
                }
            }
        });
        Self {
            queue,
            #[cfg(feature = "colour-replies")]
            replies_waiting,
        }
    }

    /// Queue typed input. Never blocks.
    ///
    /// # Errors
    ///
    /// `WouldBlock` if the program has left too much unread, `BrokenPipe` if the
    /// writer has stopped.
    pub(crate) fn typed(&self, bytes: &[u8]) -> io::Result<()> {
        match self.queue.try_send(Message::Typed(bytes.to_vec())) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "the program is not reading its input",
            )),
            Err(TrySendError::Disconnected(_)) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the PTY writer has stopped",
            )),
        }
    }

    /// Queue one of the terminal's own replies. Never blocks. Returns whether it
    /// was queued.
    #[cfg(feature = "colour-replies")]
    pub(crate) fn reply(&self, bytes: Vec<u8>) -> bool {
        if self.replies_waiting.fetch_add(1, Ordering::SeqCst) >= REPLY_QUEUE {
            self.replies_waiting.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        if self.queue.try_send(Message::Reply(bytes)).is_ok() {
            true
        } else {
            self.replies_waiting.fetch_sub(1, Ordering::SeqCst);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A writer that waits for a release before it takes each write, as a PTY
    /// does when the program is not reading, and records what it was given.
    struct Stalled {
        release: Receiver<()>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.release.recv().is_err() {
                return Err(io::Error::other("released for good"));
            }
            if let Ok(mut w) = self.written.lock() {
                w.extend_from_slice(bytes);
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn stalled() -> (InputWriter, Sender<()>, Arc<Mutex<Vec<u8>>>) {
        let (release, gate) = channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = InputWriter::new(Box::new(Stalled {
            release: gate,
            written: Arc::clone(&written),
        }));
        (writer, release, written)
    }

    #[cfg(feature = "colour-replies")]
    #[test]
    fn replies_are_dropped_when_sixteen_wait_and_never_block() {
        let (w, _release, _) = stalled();
        let start = Instant::now();
        let queued = (0..1000).filter(|_| w.reply(b"r".to_vec())).count();
        assert!(start.elapsed() < Duration::from_secs(1), "queueing blocked");
        // One is in the stalled write, and REPLY_QUEUE wait behind it.
        assert!(
            (REPLY_QUEUE..=REPLY_QUEUE + 1).contains(&queued),
            "{queued} queued"
        );
    }

    #[cfg(feature = "colour-replies")]
    #[test]
    fn a_full_run_of_replies_leaves_the_room_typed_input_is_promised() {
        let (w, _release, _) = stalled();
        while w.reply(b"r".to_vec()) {}
        let start = Instant::now();
        for i in 0..TYPED_QUEUE {
            assert!(w.typed(b"k").is_ok(), "key {i} was refused");
        }
        assert!(start.elapsed() < Duration::from_secs(1), "queueing blocked");
    }

    #[test]
    fn typed_input_never_blocks_when_the_program_is_not_reading() {
        let (w, _release, _) = stalled();
        let start = Instant::now();
        let mut accepted = 0;
        let mut refused = 0;
        for _ in 0..(TYPED_QUEUE * 4) {
            match w.typed(b"k") {
                Ok(()) => accepted += 1,
                Err(e) => {
                    assert_eq!(e.kind(), io::ErrorKind::WouldBlock);
                    refused += 1;
                }
            }
        }
        assert!(start.elapsed() < Duration::from_secs(1), "queueing blocked");
        assert!(accepted >= TYPED_QUEUE, "{accepted} accepted");
        assert!(refused > 0, "a stalled program is eventually refused");
    }

    #[test]
    fn what_was_queued_is_written_in_order_once_the_program_reads() {
        let (w, release, written) = stalled();
        for chunk in [&b"one "[..], b"two ", b"three"] {
            assert!(w.typed(chunk).is_ok());
        }
        for _ in 0..3 {
            assert!(release.send(()).is_ok());
        }
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if written
                .lock()
                .is_ok_and(|v| v.as_slice() == b"one two three")
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unreachable!("the queued input was not written in order");
    }

    #[test]
    fn a_stopped_writer_refuses_with_broken_pipe() {
        let (w, release, _) = stalled();
        drop(release);
        assert!(w.typed(b"x").is_ok(), "the first write is accepted");
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if let Err(e) = w.typed(b"x") {
                assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unreachable!("the stopped writer kept accepting input");
    }
}
