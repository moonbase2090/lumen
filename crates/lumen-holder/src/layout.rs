//! The descriptors a holder is started with.

use std::os::fd::RawFd;

/// The seal channel: the shell integration asks the holder to seal and open
/// frames here.
pub const SEAL_FD: RawFd = 3;
/// The spawn socketpair: the terminal delivers the key here.
pub const SPAWN_FD: RawFd = 4;
/// The read end of the EOF pipe. The terminal holds the only write end, so
/// the pipe reaches EOF when the terminal dies.
pub const EOF_FD: RawFd = 5;
/// A pidfd for the shell, from the terminal's own spawn of it.
pub const PIDFD_FD: RawFd = 6;

/// The four, in order.
pub const ALL: [RawFd; 4] = [SEAL_FD, SPAWN_FD, EOF_FD, PIDFD_FD];
