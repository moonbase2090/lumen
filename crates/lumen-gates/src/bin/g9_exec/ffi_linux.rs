//! Linux descriptor and `execveat` calls used only by the G9 process roles.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::RawFd;

/// Mark an inherited descriptor close-on-exec, as the shell shim does.
pub fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: `F_GETFD` reads the flags for one descriptor and touches no memory.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `F_SETFD` writes the flags for one descriptor and touches no memory.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Replace this process with `program` through `execveat(AT_FDCWD, ...)`.
pub fn execveat(program: &CStr, arguments: &[CString]) -> io::Error {
    let mut argv: Vec<*const libc::c_char> = arguments.iter().map(|s| s.as_ptr()).collect();
    argv.push(std::ptr::null());
    let envp = [std::ptr::null::<libc::c_char>()];
    // SAFETY: `program`, every `argv` string and both pointer arrays remain
    // live through the call and are terminated as `execveat` requires. On
    // success the process image is replaced; on failure the syscall returns.
    unsafe {
        libc::syscall(
            libc::SYS_execveat,
            libc::AT_FDCWD,
            program.as_ptr(),
            argv.as_ptr(),
            envp.as_ptr(),
            0,
        );
    }
    io::Error::last_os_error()
}
