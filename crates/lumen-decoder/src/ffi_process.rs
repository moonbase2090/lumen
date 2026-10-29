//! Signalling a worker's process group. The only `unsafe` the host side needs.
#![allow(unsafe_code)]

use std::io;

/// Kills every process in the process group led by `pid` with SIGKILL.
///
/// A group that no longer exists is not an error.
///
/// # Errors
/// Any error from `killpg` other than "no such process".
pub fn kill_group(pid: u32) -> io::Result<()> {
    let group = i32::try_from(pid).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `killpg` takes two integers and reads no memory. A stale group id
    // can only make it fail with ESRCH; the worker is its own group leader and
    // has not been reaped yet when this is called, so its id is not reused.
    let rc = unsafe { libc::killpg(group, libc::SIGKILL) };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(err)
    }
}
