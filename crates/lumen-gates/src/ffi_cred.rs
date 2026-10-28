//! Socket-credential primitives for gate G1.
//!
//! The FFI module for the seal channel experiment. Every call works on a
//! borrowed file descriptor or an owned one, and every control-message buffer
//! is a local, aligned array, so no pointer leaves this module.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the casts are of small constant sizes and of byte counts already checked non-negative"
)]

use std::io;
use std::mem;
use std::os::fd::{AsFd, AsRawFd};
#[cfg(target_os = "linux")]
use std::os::fd::{FromRawFd, OwnedFd};

/// A process identity as the kernel reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Creds {
    /// Process id.
    pub pid: i32,
    /// User id.
    pub uid: u32,
    /// Group id.
    pub gid: u32,
}

/// Room for one credentials control message, aligned for `cmsghdr`.
#[cfg(target_os = "linux")]
const CONTROL_WORDS: usize = 8;

/// A connected, message-oriented Unix socket pair with close-on-exec set.
///
/// # Errors
///
/// Returns the OS error if `socketpair` fails.
#[cfg(target_os = "linux")]
pub fn socketpair_seqpacket() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a writable array of two ints, as `socketpair` requires.
    let rc = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `socketpair` succeeded, so both descriptors are open and owned
    // by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Ask the kernel to attach the sender's credentials to every message received
/// on `fd`.
///
/// # Errors
///
/// Returns the OS error if `setsockopt` fails.
#[cfg(target_os = "linux")]
pub fn enable_passcred(fd: &impl AsFd) -> io::Result<()> {
    let on: libc::c_int = 1;
    // SAFETY: `on` is a valid int and its size is passed.
    let rc = unsafe {
        libc::setsockopt(
            fd.as_fd().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&raw const on).cast(),
            mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Send one message. With `claim`, send an explicit credentials control
/// message carrying those values; the kernel accepts it only if it matches the
/// real sender (or the sender is privileged).
///
/// # Errors
///
/// Returns the OS error if `sendmsg` fails, for example `EPERM` for a claim
/// that does not match the sender.
#[cfg(target_os = "linux")]
pub fn send(fd: &impl AsFd, data: &[u8], claim: Option<Creds>) -> io::Result<usize> {
    let mut iov = libc::iovec {
        iov_base: data.as_ptr().cast_mut().cast(),
        iov_len: data.len(),
    };
    let mut control = [0u64; CONTROL_WORDS];
    // SAFETY: an all-zero `msghdr` is a valid empty message header.
    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    if let Some(c) = claim {
        let ucred = libc::ucred {
            pid: c.pid,
            uid: c.uid,
            gid: c.gid,
        };
        // SAFETY: `CMSG_SPACE` and `CMSG_LEN` are pure size computations.
        let (space, len) = unsafe {
            (
                libc::CMSG_SPACE(mem::size_of::<libc::ucred>() as u32),
                libc::CMSG_LEN(mem::size_of::<libc::ucred>() as u32),
            )
        };
        if space as usize > mem::size_of_val(&control) {
            return Err(io::Error::other("control buffer too small"));
        }
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        // SAFETY: `msg_control` points at `control`, which is aligned and at
        // least `space` bytes, so the first header and its data fit.
        unsafe {
            let hdr = libc::CMSG_FIRSTHDR(&raw const msg);
            (*hdr).cmsg_level = libc::SOL_SOCKET;
            (*hdr).cmsg_type = libc::SCM_CREDENTIALS;
            (*hdr).cmsg_len = len as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(hdr).cast::<libc::ucred>(), ucred);
        }
    }
    // SAFETY: `msg` references `iov` and `control`, both alive for the call,
    // and `iov` describes `data`, which `sendmsg` only reads.
    let n = unsafe { libc::sendmsg(fd.as_fd().as_raw_fd(), &raw const msg, 0) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Receive one message into `buf`. The credentials are those the kernel
/// attached to this message, or `None` if none arrived.
///
/// # Errors
///
/// Returns the OS error if `recvmsg` fails.
#[cfg(target_os = "linux")]
pub fn recv(fd: &impl AsFd, buf: &mut [u8]) -> io::Result<(usize, Option<Creds>)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    let mut control = [0u64; CONTROL_WORDS];
    // SAFETY: an all-zero `msghdr` is a valid empty message header.
    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = mem::size_of_val(&control) as _;
    // SAFETY: `msg` references `iov` and `control`, both alive and writable.
    let n = unsafe { libc::recvmsg(fd.as_fd().as_raw_fd(), &raw mut msg, libc::MSG_CMSG_CLOEXEC) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut found = None;
    // SAFETY: the kernel filled `msg_control` with well-formed control
    // messages; the walk uses the `CMSG_*` helpers and stays inside them.
    unsafe {
        let mut hdr = libc::CMSG_FIRSTHDR(&raw const msg);
        while !hdr.is_null() {
            if (*hdr).cmsg_level == libc::SOL_SOCKET && (*hdr).cmsg_type == libc::SCM_CREDENTIALS {
                let u = std::ptr::read_unaligned(libc::CMSG_DATA(hdr).cast::<libc::ucred>());
                found = Some(Creds {
                    pid: u.pid,
                    uid: u.uid,
                    gid: u.gid,
                });
            }
            hdr = libc::CMSG_NXTHDR(&raw const msg, hdr);
        }
    }
    Ok((n as usize, found))
}

/// The process id the kernel recorded for the other end of `fd` when the
/// connection was made. On Linux this is `SO_PEERCRED`; on macOS it is
/// `LOCAL_PEERPID`. Neither follows the process that later writes.
///
/// # Errors
///
/// Returns the OS error if `getsockopt` fails.
pub fn peer_pid(fd: &impl AsFd) -> io::Result<i32> {
    #[cfg(target_os = "linux")]
    {
        let mut u = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `u` is a writable `ucred` and `len` holds its size.
        let rc = unsafe {
            libc::getsockopt(
                fd.as_fd().as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut u).cast(),
                &raw mut len,
            )
        };
        if rc == 0 {
            Ok(u.pid)
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: `pid` is a writable pid_t and `len` holds its size.
        let rc = unsafe {
            libc::getsockopt(
                fd.as_fd().as_raw_fd(),
                0, // SOL_LOCAL
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &raw mut len,
            )
        };
        if rc == 0 {
            Ok(pid)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// The effective group id of this process.
#[must_use]
pub fn effective_gid() -> u32 {
    // SAFETY: `getegid` takes no arguments, has no memory effects and cannot fail.
    unsafe { libc::getegid() }
}

/// Take ownership of descriptor 0, which the parent passed as the socket.
///
/// Call once, at the start of a role that was started with the socket as its
/// stdin; a second call would double-own the descriptor.
#[must_use]
pub fn stdin_owned() -> std::os::fd::OwnedFd {
    // SAFETY: descriptor 0 is open for the life of the process, and the
    // caller takes it exactly once, so no other owner closes it.
    unsafe { std::os::fd::FromRawFd::from_raw_fd(0) }
}
