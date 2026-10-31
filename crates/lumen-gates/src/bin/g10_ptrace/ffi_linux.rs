//! Linux process-inspection calls used by the G10 matrix.

use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

fn last_if_negative(result: libc::c_long) -> io::Result<libc::c_long> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Attach to `pid`, wait for its stop, then detach it.
pub fn ptrace_round_trip(pid: u32) -> io::Result<()> {
    // SAFETY: `PTRACE_ATTACH` takes a pid and ignores both pointer arguments.
    last_if_negative(unsafe {
        libc::ptrace(
            libc::PTRACE_ATTACH,
            pid as libc::pid_t,
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
        ) as libc::c_long
    })?;
    let mut status = 0;
    // SAFETY: `status` is a live integer for `waitpid` to fill.
    if unsafe { libc::waitpid(pid as libc::pid_t, &raw mut status, libc::__WALL) } < 0 {
        let error = io::Error::last_os_error();
        // SAFETY: best-effort release after a successful attach.
        unsafe {
            libc::ptrace(
                libc::PTRACE_DETACH,
                pid as libc::pid_t,
                std::ptr::null_mut::<libc::c_void>(),
                std::ptr::null_mut::<libc::c_void>(),
            );
        }
        return Err(error);
    }
    // SAFETY: the successful attach and wait make this process the tracer;
    // `PTRACE_DETACH` takes no data pointers here.
    last_if_negative(unsafe {
        libc::ptrace(
            libc::PTRACE_DETACH,
            pid as libc::pid_t,
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
        ) as libc::c_long
    })?;
    Ok(())
}

/// Read one `u64` from another process through `process_vm_readv`.
pub fn process_vm_read(pid: u32, address: usize) -> io::Result<u64> {
    let mut value = 0u64;
    let local = libc::iovec {
        iov_base: (&raw mut value).cast(),
        iov_len: std::mem::size_of::<u64>(),
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: std::mem::size_of::<u64>(),
    };
    // SAFETY: the local vector names the live `value`; the remote vector is
    // untrusted input to the kernel, which reports an error if it is invalid.
    let read = unsafe {
        libc::process_vm_readv(
            pid as libc::pid_t,
            &raw const local,
            1,
            &raw const remote,
            1,
            0,
        )
    };
    if read < 0 {
        return Err(io::Error::last_os_error());
    }
    if read as usize != std::mem::size_of::<u64>() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "process_vm_readv returned a short value",
        ));
    }
    Ok(value)
}

/// Open a pidfd for `pid`.
pub fn pidfd_open(pid: u32) -> io::Result<OwnedFd> {
    // SAFETY: the syscall takes only integer arguments and returns a new fd.
    let fd = last_if_negative(unsafe {
        libc::syscall(libc::SYS_pidfd_open, pid as libc::c_long, 0 as libc::c_long)
    })?;
    // SAFETY: the syscall just returned this descriptor and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

/// Duplicate `target_fd` from the process named by `pidfd`.
pub fn pidfd_getfd(pidfd: RawFd, target_fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: the syscall takes only descriptor numbers and flags and returns
    // a new descriptor without dereferencing user memory.
    let fd = last_if_negative(unsafe {
        libc::syscall(
            libc::SYS_pidfd_getfd,
            pidfd as libc::c_long,
            target_fd as libc::c_long,
            0 as libc::c_long,
        )
    })?;
    // SAFETY: the syscall just returned this descriptor and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}
