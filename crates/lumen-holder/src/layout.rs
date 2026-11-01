//! The descriptors a holder is started with.

/// The seal channel: the shell integration asks the holder to seal and open
/// frames here.
pub const SEAL_FD: std::os::raw::c_int = 3;
/// The spawn socketpair: the terminal delivers the key here.
pub const SPAWN_FD: std::os::raw::c_int = 4;
/// The read end of the EOF pipe. The terminal holds the only write end, so
/// the pipe reaches EOF when the terminal dies.
pub const EOF_FD: std::os::raw::c_int = 5;
/// A pidfd for the shell, from the terminal's own spawn of it.
pub const PIDFD_FD: std::os::raw::c_int = 6;

/// The four, in order.
pub const ALL: [std::os::raw::c_int; 4] = [SEAL_FD, SPAWN_FD, EOF_FD, PIDFD_FD];
