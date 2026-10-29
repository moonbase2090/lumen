//! What the holder does: harden itself, then wait to be told to end.

use crate::ffi_linux::{self, Channel};
use crate::layout::{ALL, EOF_FD, PIDFD_FD, SEAL_FD};
use std::io;
use std::os::fd::RawFd;
use zeroize::Zeroizing;

/// Why the holder ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// The terminal's EOF pipe closed: the terminal is gone.
    TerminalGone,
    /// The shell's pidfd became readable: the shell exited.
    ShellExited,
    /// The seal channel closed: nothing can ask for a seal any more.
    SealChannelClosed,
    /// Something arrived on the seal channel, and no request is defined yet.
    UnexpectedRequest,
}

impl End {
    /// The exit status of the holder process for this end.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            End::TerminalGone => 10,
            End::ShellExited => 11,
            End::SealChannelClosed => 12,
            End::UnexpectedRequest => 13,
        }
    }
}

/// The exit status when the holder could not harden or wait.
pub const FAULT: u8 = 70;

/// The layer's key. It is zeroized when dropped, which is when the holder
/// ends. No key is delivered in this piece, so the holder has none.
pub struct Key(
    #[allow(dead_code, reason = "held for its drop; delivery comes later")] Zeroizing<[u8; 32]>,
);

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

/// Wait until the holder has to end, and say why.
///
/// # Errors
///
/// Returns an error from `poll` or `recv`, or `InvalidInput` if one of the
/// four descriptors is not open.
pub fn wait_for_end() -> io::Result<End> {
    loop {
        let events = ffi_linux::wait_readable(&[EOF_FD, PIDFD_FD, SEAL_FD])?;
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
        if events[2].readable || events[2].hangup {
            match ffi_linux::peek_channel(SEAL_FD)? {
                Channel::Closed => return Ok(End::SealChannelClosed),
                Channel::Message => return Ok(End::UnexpectedRequest),
                Channel::Idle => {}
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
pub fn run(terminal: u32) -> io::Result<End> {
    harden(terminal, &[])?;
    let key: Option<Key> = None;
    let end = wait_for_end()?;
    drop(key);
    Ok(end)
}
