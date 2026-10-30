//! What the holder does: harden itself, then wait to be told to end.

use crate::ffi_linux::{self, Received};
use crate::layer::{Counters, Layer};
use crate::layout::{ALL, EOF_FD, PIDFD_FD, SEAL_FD, SPAWN_FD};
use crate::principal::{self, Principal, Terminal};
use crate::protocol::MAX_MESSAGE;
use std::io;
use std::os::fd::RawFd;

/// Why the holder ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// The terminal's EOF pipe closed: the terminal is gone.
    TerminalGone,
    /// The shell's pidfd became readable: the shell exited.
    ShellExited,
    /// The seal channel closed: nothing can ask for a seal any more.
    SealChannelClosed,
}

impl End {
    /// The exit status of the holder process for this end.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            End::TerminalGone => 10,
            End::ShellExited => 11,
            End::SealChannelClosed => 12,
        }
    }
}

/// The exit status when the holder could not harden or wait.
pub const FAULT: u8 = 70;

/// Put the process in the state it runs in: not dumpable, killed with its
/// parent, with only the four descriptors it was given, and unable to fork,
/// exec or start a thread. `terminal` is the pid of the terminal that started
/// it. `also_keep` names descriptors to leave open besides the four, which is
/// empty for a holder and is standard output for the hardening check.
///
/// # Errors
///
/// Returns the first step that failed. A holder that cannot harden must not
/// run, so the caller exits.
pub fn harden(terminal: u32, also_keep: &[RawFd]) -> io::Result<()> {
    ffi_linux::set_dumpable(false)?;
    if ffi_linux::dumpable()? {
        return Err(io::Error::other("still dumpable after PR_SET_DUMPABLE"));
    }
    ffi_linux::die_with_parent(terminal)?;
    let mut keep = ALL.to_vec();
    keep.extend_from_slice(also_keep);
    ffi_linux::close_all_except(&keep)?;
    ffi_linux::deny_fork_and_exec()
}

/// Wait until the holder has to end, and say why, taking and judging the
/// messages that arrive on the seal channel meanwhile.
///
/// # Errors
///
/// Returns an error from `poll` or `recvmsg`, or `InvalidInput` if one of the
/// four descriptors is not open.
pub fn wait_for_end(layer: &mut Layer) -> io::Result<End> {
    // The terminal may close its end of the spawn socketpair once it has
    // delivered the key; that is not an end, so the descriptor stops being watched.
    let mut spawn_open = true;
    loop {
        let mut watched = vec![EOF_FD, PIDFD_FD, SEAL_FD];
        if spawn_open {
            watched.push(SPAWN_FD);
        }
        let events = ffi_linux::wait_readable(&watched)?;
        if events.iter().any(|e| e.invalid) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a holder descriptor is not open",
            ));
        }
        if events[0].readable || events[0].hangup {
            return Ok(End::TerminalGone);
        }
        if events[1].readable || events[1].hangup {
            return Ok(End::ShellExited);
        }
        if spawn_open && (events[3].readable || events[3].hangup) {
            loop {
                match ffi_linux::receive(SPAWN_FD, MAX_MESSAGE)? {
                    Received::Closed => {
                        spawn_open = false;
                        break;
                    }
                    Received::Empty => break,
                    Received::Message {
                        bytes,
                        truncated,
                        creds,
                    } => {
                        layer.handle_spawn(creds, &bytes, truncated, &|key| {
                            ffi_linux::lock_memory(key).is_ok()
                        });
                    }
                }
            }
        }
        if events[2].readable || events[2].hangup {
            // Everything queued is judged before the end of the channel is acted on.
            loop {
                match ffi_linux::receive(SEAL_FD, MAX_MESSAGE)? {
                    Received::Closed => return Ok(End::SealChannelClosed),
                    Received::Empty => break,
                    Received::Message {
                        bytes,
                        truncated,
                        creds,
                    } => {
                        layer.handle(creds, &bytes, truncated, &principal::start_time);
                        if let Some(reply) = layer.take_reply()
                            && !ffi_linux::send(SEAL_FD, &reply)?
                        {
                            layer.note_reply_dropped();
                        }
                    }
                }
            }
        }
    }
}

/// Harden, wait, and drop the key. The process then exits with the code of
/// the returned end.
///
/// # Errors
///
/// Returns why the holder could not harden or wait.
pub fn run(terminal: u32, shell: Principal, report: bool) -> io::Result<(End, Counters)> {
    ffi_linux::enable_passcred(SEAL_FD)?;
    ffi_linux::enable_passcred(SPAWN_FD)?;
    harden(terminal, if report { &[1] } else { &[] })?;
    let mut layer = Layer::new(
        shell,
        Terminal {
            pid: terminal,
            uid: shell.uid,
        },
    );
    let end = wait_for_end(&mut layer)?;
    Ok((end, layer.counters()))
}
