//! The system calls the holder and its spawner need.
//!
//! The one FFI module in this crate. Each wrapper is one call (or one small
//! sequence) that returns an `io::Result`, and each `unsafe` block says why it
//! is sound. Nothing here hands out a raw pointer.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    reason = "the system calls take C integer types of fixed, small values"
)]

use crate::principal::Creds;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use zeroize::Zeroizing;

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Turn dumping on or off (`PR_SET_DUMPABLE`). Off, a process of the same user
/// cannot attach to it or read its memory through `/proc`.
///
/// # Errors
///
/// Returns the error `prctl` reports.
pub fn set_dumpable(on: bool) -> io::Result<()> {
    // SAFETY: `PR_SET_DUMPABLE` takes one integer and touches no memory.
    check(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, libc::c_ulong::from(on), 0, 0, 0) }).map(drop)
}

/// Whether the process is dumpable (`PR_GET_DUMPABLE`).
///
/// # Errors
///
/// Returns the error `prctl` reports.
pub fn dumpable() -> io::Result<bool> {
    // SAFETY: `PR_GET_DUMPABLE` takes no argument and touches no memory.
    check(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) }).map(|v| v == 1)
}

/// Ask the kernel to kill this process when its parent dies, and fail if the
/// parent is no longer `parent`, so a parent that died first is not missed.
///
/// The kernel ties the signal to the parent *thread* that started the process,
/// not to the parent process: it is sent when that thread exits. A terminal
/// must start holders from a thread that lives as long as the terminal.
///
/// # Errors
///
/// Returns the error `prctl` reports, or `BrokenPipe` if the parent is gone.
pub fn die_with_parent(parent: u32) -> io::Result<()> {
    // SAFETY: `PR_SET_PDEATHSIG` takes one signal number and touches no memory.
    check(unsafe {
        libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGKILL as libc::c_ulong,
            0,
            0,
            0,
        )
    })?;
    // SAFETY: `getppid` takes no arguments, has no memory effects and cannot fail.
    if unsafe { libc::getppid() } as u32 == parent {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the parent is gone",
        ))
    }
}

/// The parent's pid.
#[must_use]
pub fn parent_pid() -> u32 {
    // SAFETY: `getppid` takes no arguments, has no memory effects and cannot fail.
    unsafe { libc::getppid() as u32 }
}

/// Forbid gaining privileges through `execve` (`PR_SET_NO_NEW_PRIVS`), which a
/// seccomp filter needs.
///
/// # Errors
///
/// Returns the error `prctl` reports.
pub fn no_new_privs() -> io::Result<()> {
    // SAFETY: `PR_SET_NO_NEW_PRIVS` takes one integer and touches no memory.
    check(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }).map(drop)
}

/// Close every descriptor except those in `keep`.
///
/// # Errors
///
/// Returns the error from listing `/proc/self/fd`.
pub fn close_all_except(keep: &[RawFd]) -> io::Result<()> {
    let listing: Vec<RawFd> = std::fs::read_dir("/proc/self/fd")?
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .collect();
    for fd in listing {
        if !keep.contains(&fd) {
            // SAFETY: closing a descriptor number. The directory handle used
            // for the listing is already closed, so its number, if listed, is
            // either closed already (`EBADF`, ignored) or about to be.
            unsafe { libc::close(fd) };
        }
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xc000_00b7;

#[cfg(target_arch = "x86_64")]
const DENIED: &[libc::c_long] = &[
    libc::SYS_fork,
    libc::SYS_vfork,
    libc::SYS_clone,
    libc::SYS_clone3,
    libc::SYS_execve,
    libc::SYS_execveat,
];
#[cfg(target_arch = "aarch64")]
const DENIED: &[libc::c_long] = &[
    libc::SYS_clone,
    libc::SYS_clone3,
    libc::SYS_execve,
    libc::SYS_execveat,
];

const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_RET_K: u16 = 0x06;
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ALLOW: u32 = 0x7fff_0000;
const RET_ERRNO: u32 = 0x0005_0000;

fn statement(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

/// The filter: another architecture's call numbers kill the process, the
/// denied calls fail with `EPERM`, everything else is allowed.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn fork_exec_filter() -> Vec<libc::sock_filter> {
    let denied = u8::try_from(DENIED.len()).unwrap_or(u8::MAX);
    let mut program = vec![
        statement(BPF_LD_W_ABS, 4),
        libc::sock_filter {
            code: BPF_JEQ_K,
            jt: 1,
            jf: 0,
            k: AUDIT_ARCH,
        },
        statement(BPF_RET_K, RET_KILL_PROCESS),
        statement(BPF_LD_W_ABS, 0),
    ];
    for (i, nr) in DENIED.iter().enumerate() {
        let i = u8::try_from(i).unwrap_or(u8::MAX);
        program.push(libc::sock_filter {
            code: BPF_JEQ_K,
            jt: denied - i,
            jf: 0,
            k: u32::try_from(*nr).unwrap_or(u32::MAX),
        });
    }
    program.push(statement(BPF_RET_K, RET_ALLOW));
    program.push(statement(BPF_RET_K, RET_ERRNO | libc::EPERM as u32));
    program
}

/// From now on `fork`, `vfork`, `clone`, `clone3`, `execve` and `execveat`
/// fail with `EPERM` for this process and everything it could start, so the
/// holder can neither fork nor exec nor start a thread. A call from another
/// architecture's table kills the process.
///
/// # Errors
///
/// Returns the error `prctl` reports, or `Unsupported` on an architecture the
/// filter has no call numbers for.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn deny_fork_and_exec() -> io::Result<()> {
    no_new_privs()?;
    let mut program = fork_exec_filter();
    let fprog = libc::sock_fprog {
        len: u16::try_from(program.len()).unwrap_or(u16::MAX),
        filter: program.as_mut_ptr(),
    };
    // SAFETY: `fprog` points at `program`, which lives until the call returns;
    // the kernel copies the filter before `prctl` returns.
    check(unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER as libc::c_ulong,
            &raw const fprog,
            0,
            0,
        )
    })
    .map(drop)
}

/// The filter has no call numbers for this architecture.
///
/// # Errors
///
/// Always `Unsupported`.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn deny_fork_and_exec() -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Lock the memory of `bytes` so it is not written to swap.
///
/// # Errors
///
/// Returns the error `mlock` reports (commonly a limit).
pub fn lock_memory(bytes: &[u8]) -> io::Result<()> {
    // SAFETY: the pointer and length are those of a live slice, and `mlock`
    // only pins the pages that hold it.
    check(unsafe { libc::mlock(bytes.as_ptr().cast(), bytes.len()) }).map(drop)
}

/// What `poll` said about one descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Events {
    /// Readable, or at its end.
    pub readable: bool,
    /// The other end hung up.
    pub hangup: bool,
    /// Not an open descriptor.
    pub invalid: bool,
}

/// Wait until one of `fds` is readable or hung up, with no time limit.
///
/// # Errors
///
/// Returns the error `poll` reports, other than an interrupt, which is retried.
pub fn wait_readable(fds: &[RawFd]) -> io::Result<Vec<Events>> {
    wait_with_timeout(fds, -1)
}

/// [`wait_readable`] with a time limit in milliseconds (0 returns at once, -1
/// waits without a limit).
///
/// # Errors
///
/// As [`wait_readable`].
pub fn wait_with_timeout(fds: &[RawFd], millis: libc::c_int) -> io::Result<Vec<Events>> {
    let mut set: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    loop {
        // SAFETY: `set` is a live array of `set.len()` valid `pollfd`s for the
        // whole call, and -1 waits without a time limit.
        let n = unsafe { libc::poll(set.as_mut_ptr(), set.len() as libc::nfds_t, millis) };
        match check(n) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(set
        .iter()
        .map(|p| Events {
            readable: p.revents & libc::POLLIN != 0,
            hangup: p.revents & libc::POLLHUP != 0,
            invalid: p.revents & libc::POLLNVAL != 0,
        })
        .collect())
}

/// What `recvmsg` returned on a message channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// The other end is closed (a read returns 0).
    Closed,
    /// Nothing is waiting.
    Empty,
    /// One datagram.
    Message {
        /// The bytes that fit in the buffer. Zeroized when dropped, since the
        /// spawn socketpair carries the key.
        bytes: Zeroizing<Vec<u8>>,
        /// The datagram was longer than the buffer and was cut.
        truncated: bool,
        /// The sender the kernel recorded for it, if any.
        creds: Option<Creds>,
    },
}

/// Ask the kernel to attach the sender's credentials to every message received
/// on `fd` (`SO_PASSCRED`). The option belongs to the socket, so it holds in
/// every process that has it open.
///
/// # Errors
///
/// Returns the error `setsockopt` reports.
pub fn enable_passcred(fd: RawFd) -> io::Result<()> {
    let on: libc::c_int = 1;
    // SAFETY: `on` is a live int and its size is passed.
    check(unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&raw const on).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    })
    .map(drop)
}

/// Take one datagram of at most `capacity` bytes from `fd` without blocking,
/// with the credentials the kernel attached to it. A zero-length read is the
/// other end closing; no message of length zero is defined.
///
/// # Errors
///
/// Returns the error `recvmsg` reports other than "would block".
pub fn receive(fd: RawFd, capacity: usize) -> io::Result<Received> {
    let mut buffer = Zeroizing::new(vec![0u8; capacity]);
    let mut iov = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let mut control = [0u64; 32];
    // SAFETY: an all-zero `msghdr` is a valid empty header.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control) as _;
    // SAFETY: `msg` references `iov` and `control`, both alive and writable for
    // the call; `MSG_CMSG_CLOEXEC` closes any descriptor a sender tried to pass.
    let n = unsafe {
        libc::recvmsg(
            fd,
            &raw mut msg,
            libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC,
        )
    };
    if n < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::WouldBlock {
            Ok(Received::Empty)
        } else {
            Err(e)
        };
    }
    if n == 0 {
        return Ok(Received::Closed);
    }
    let mut creds = None;
    // SAFETY: the kernel filled `msg_control` with well-formed control
    // messages; the walk uses the `CMSG_*` helpers and stays inside them.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&raw const msg);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET
                && (*header).cmsg_type == libc::SCM_CREDENTIALS
            {
                let u = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<libc::ucred>());
                creds = Some(Creds {
                    pid: u.pid,
                    uid: u.uid,
                    gid: u.gid,
                });
            }
            header = libc::CMSG_NXTHDR(&raw const msg, header);
        }
    }
    buffer.truncate(n as usize);
    Ok(Received::Message {
        bytes: buffer,
        truncated: msg.msg_flags & libc::MSG_TRUNC != 0,
        creds,
    })
}

/// Send one datagram on `fd` without blocking. `Ok(false)` means it was not
/// sent: the peer's queue is full or the peer is gone. Nothing is retried; the
/// caller counts it.
///
/// # Errors
///
/// Returns the error `send` reports other than a full queue or a closed peer.
pub fn send(fd: RawFd, bytes: &[u8]) -> io::Result<bool> {
    // SAFETY: the pointer and length are those of a live slice, and the call
    // only reads from it. `MSG_NOSIGNAL` keeps a closed peer from raising a signal.
    let n = unsafe {
        libc::send(
            fd,
            bytes.as_ptr().cast(),
            bytes.len(),
            libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
        )
    };
    if n >= 0 {
        return Ok(n as usize == bytes.len());
    }
    let e = io::Error::last_os_error();
    match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset => {
            Ok(false)
        }
        _ => Err(e),
    }
}

/// The effective user id of this process.
#[must_use]
pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments, has no memory effects and cannot fail.
    unsafe { libc::geteuid() }
}

/// A pidfd for `pid` (`pidfd_open`).
///
/// # Errors
///
/// Returns the error the call reports.
pub fn pidfd_open(pid: u32) -> io::Result<OwnedFd> {
    // SAFETY: `pidfd_open` takes a pid and flags and returns a new descriptor.
    let fd =
        check(
            unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::c_long, 0 as libc::c_long) }
                as libc::c_int,
        )?;
    // SAFETY: `fd` is a descriptor this call just received and nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Whether `pidfd` is readable: the process it names has exited.
///
/// # Errors
///
/// Returns the error `poll` reports, or `InvalidInput` for a descriptor that
/// is not open.
pub fn pidfd_is_readable(pidfd: RawFd) -> io::Result<bool> {
    let events = wait_with_timeout(&[pidfd], 0)?;
    if events[0].invalid {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    Ok(events[0].readable || events[0].hangup)
}

/// A connected pair of `SOCK_SEQPACKET` Unix sockets, close-on-exec.
///
/// # Errors
///
/// Returns the error `socketpair` reports.
pub fn seqpacket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: `ends` is an array of two integers, as `socketpair` requires.
    check(unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            ends.as_mut_ptr(),
        )
    })?;
    // SAFETY: both are descriptors the call just returned and nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) })
}

/// A pipe as (read end, write end), both close-on-exec.
///
/// # Errors
///
/// Returns the error `pipe2` reports.
pub fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: `ends` is an array of two integers, as `pipe2` requires.
    check(unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) })?;
    // SAFETY: both are descriptors the call just returned and nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) })
}

/// In a child between `fork` and `exec`: put `sources[i]` at descriptor
/// `targets[i]` without a source ever being overwritten before it is copied.
/// Uses only `fcntl`, `dup2` and `close`, which are safe there.
///
/// # Errors
///
/// Returns the error of the call that failed.
pub fn place_descriptors(sources: &[RawFd], targets: &[RawFd]) -> io::Result<()> {
    debug_assert_eq!(sources.len(), targets.len(), "one target for each source");
    debug_assert!(sources.len() <= 8, "at most eight descriptors are placed");
    let mut moved = [0 as RawFd; 8];
    let top = targets.iter().copied().max().unwrap_or(0) + 1;
    for (i, &source) in sources.iter().enumerate().take(moved.len()) {
        // SAFETY: `fcntl` with `F_DUPFD_CLOEXEC` copies a descriptor to one at or
        // above `top`, which no target can be.
        moved[i] = check(unsafe { libc::fcntl(source, libc::F_DUPFD_CLOEXEC, top) })?;
    }
    for (i, &target) in targets.iter().enumerate().take(moved.len()) {
        // SAFETY: `dup2` copies a descriptor this function just made onto `target`,
        // which clears close-on-exec on the copy.
        check(unsafe { libc::dup2(moved[i], target) })?;
    }
    Ok(())
}
