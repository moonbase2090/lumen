//! The two checks the file system cannot answer.
//!
//! The one FFI module in this crate. Both wrap one system call and own no
//! memory: a user namespace has a parent unless it is the initial one, which
//! only an `ioctl` can say, and a pidfd is readable once its process has
//! exited, which only `poll` can say.

use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

/// `NS_GET_PARENT`, `_IO(0xb7, 2)`: open the parent namespace of a namespace
/// file. The initial user namespace has none and the call fails with `EPERM`.
const NS_GET_PARENT: libc::c_ulong = 0xb702;

/// Whether the user namespace at `path` (a `/proc/<pid>/ns/user`) has no
/// parent, which is what `NS_GET_PARENT` reports with `EPERM` for the initial
/// one. `None` when the file cannot be opened or the call fails any other way.
#[must_use]
pub fn user_namespace_has_no_parent(path: &Path) -> Option<bool> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: `file` is an open descriptor for the whole call. `NS_GET_PARENT`
    // takes no argument and returns a new descriptor, which is closed at once
    // if the call succeeds.
    let parent = unsafe { libc::ioctl(file.as_raw_fd(), NS_GET_PARENT) };
    if parent >= 0 {
        // SAFETY: `parent` is a descriptor this call just received and nothing else owns.
        unsafe { libc::close(parent) };
        return Some(false);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => Some(true),
        _ => None,
    }
}

/// Whether `pidfd` is still unreadable: the process it names has not exited.
/// `None` when `poll` fails or reports the descriptor invalid.
#[must_use]
pub fn pidfd_is_unreadable(pidfd: RawFd) -> Option<bool> {
    let mut fds = libc::pollfd {
        fd: pidfd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `fds` is one valid `pollfd` that lives for the whole call, and
    // the count is 1; a zero timeout makes the call return at once.
    let ready = unsafe { libc::poll(&raw mut fds, 1, 0) };
    if ready < 0 || fds.revents & libc::POLLNVAL != 0 {
        return None;
    }
    Some(ready == 0)
}
