//! Entering the sandbox. The worker calls [`enter`] before it reads a byte of
//! input; a failure here is fatal and the worker must exit.
//!
//! This is the only place in the worker that uses `unsafe`: resource limits,
//! `no_new_privs` and the seccomp filter on Linux, and the Seatbelt profile on
//! macOS are system calls with no safe wrapper in the standard library.
#![allow(unsafe_code)]

use std::io;

use crate::limits::CPU_SECONDS;

/// The type `setrlimit` takes for a resource: it differs between libc targets.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type Resource = libc::c_int;

fn set_limit(resource: Resource, soft: u64, hard: u64) -> io::Result<()> {
    let lim = libc::rlimit {
        rlim_cur: soft as libc::rlim_t,
        rlim_max: hard as libc::rlim_t,
    };
    // SAFETY: `lim` is a valid, initialised rlimit that lives across the call.
    if unsafe { libc::setrlimit(resource, &raw const lim) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Closes every descriptor above stderr.
fn close_extra_fds() {
    for fd in 3..1024 {
        // SAFETY: closing a descriptor number touches no memory; an invalid one
        // only returns EBADF, which is ignored.
        unsafe { libc::close(fd) };
    }
}

/// Applies resource limits, closes stray descriptors and enters the platform
/// sandbox.
///
/// # Errors
/// Any step that fails. There is no partial sandbox.
pub fn enter() -> io::Result<()> {
    close_extra_fds();
    set_limit(libc::RLIMIT_CORE, 0, 0)?;
    set_limit(libc::RLIMIT_CPU, CPU_SECONDS, CPU_SECONDS + 1)?;
    set_limit(libc::RLIMIT_NOFILE, 3, 3)?;
    platform::limits()?;
    platform::profile()
}

/// Enters only the platform sandbox profile (seccomp or Seatbelt), without the
/// descriptor and process limits. The sandbox suite uses it to show that the
/// profile alone denies what the limits would otherwise hide.
///
/// # Errors
/// If the profile cannot be applied.
pub fn enter_profile_only() -> io::Result<()> {
    platform::profile()
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io;

    use super::set_limit;
    use crate::limits::MEMORY_BYTES;

    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    const RET_KILL: u32 = 0x8000_0000;
    const RET_ALLOW: u32 = 0x7fff_0000;
    const LD_ABS: u16 = 0x20;
    const JEQ_K: u16 = 0x15;
    const RET_K: u16 = 0x06;

    /// The system calls the decoder may make. Everything else kills the
    /// process: no open, socket, exec, fork or clone.
    fn allowed() -> Vec<u32> {
        [
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_close,
            libc::SYS_mmap,
            libc::SYS_munmap,
            libc::SYS_mremap,
            libc::SYS_mprotect,
            libc::SYS_brk,
            libc::SYS_madvise,
            libc::SYS_futex,
            libc::SYS_rt_sigreturn,
            libc::SYS_rt_sigprocmask,
            libc::SYS_sigaltstack,
            libc::SYS_clock_gettime,
            libc::SYS_getrandom,
            libc::SYS_sched_yield,
            libc::SYS_exit,
            libc::SYS_exit_group,
        ]
        .iter()
        .map(|&n| u32::try_from(n).unwrap_or(u32::MAX))
        .collect()
    }

    fn stmt(code: u16, k: u32) -> libc::sock_filter {
        libc::sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jeq(k: u32, jt: u8) -> libc::sock_filter {
        libc::sock_filter {
            code: JEQ_K,
            jt,
            jf: 0,
            k,
        }
    }

    fn program() -> Vec<libc::sock_filter> {
        let calls = allowed();
        let n = calls.len();
        let mut p = vec![
            stmt(LD_ABS, 4), // seccomp_data.arch
            jeq(AUDIT_ARCH, 1),
            stmt(RET_K, RET_KILL),
            stmt(LD_ABS, 0), // seccomp_data.nr
        ];
        for (i, nr) in calls.iter().enumerate() {
            // Jump over the remaining compares and the kill to the allow.
            let jt = u8::try_from(n - i).unwrap_or(u8::MAX);
            p.push(jeq(*nr, jt));
        }
        p.push(stmt(RET_K, RET_KILL));
        p.push(stmt(RET_K, RET_ALLOW));
        p
    }

    pub fn limits() -> io::Result<()> {
        set_limit(libc::RLIMIT_AS, MEMORY_BYTES, MEMORY_BYTES)?;
        set_limit(libc::RLIMIT_NPROC, 0, 0)
    }

    pub fn profile() -> io::Result<()> {
        // SAFETY: PR_SET_NO_NEW_PRIVS takes integer arguments only.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut prog = program();
        let fprog = libc::sock_fprog {
            len: u16::try_from(prog.len())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
            filter: prog.as_mut_ptr(),
        };
        // SAFETY: `fprog` points at `prog`, which outlives the call; the kernel
        // copies the filter before `prctl` returns.
        let rc = unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &fprog) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CStr, CString, c_char, c_int};
    use std::io;
    use std::ptr;

    use super::set_limit;
    use crate::limits::MEMORY_BYTES;

    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    /// Deny by default: no filesystem, network, process creation or IPC.
    const PROFILE: &str = "(version 1)(deny default)";

    pub fn limits() -> io::Result<()> {
        // The kernel here refuses to set a memory limit, so these are best
        // effort; the worker's capped allocator is what holds the 512 MiB line.
        let _ = set_limit(libc::RLIMIT_AS, MEMORY_BYTES, MEMORY_BYTES);
        let _ = set_limit(libc::RLIMIT_DATA, MEMORY_BYTES, MEMORY_BYTES);
        set_limit(libc::RLIMIT_NPROC, 0, 0)
    }

    pub fn profile() -> io::Result<()> {
        let profile =
            CString::new(PROFILE).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut err: *mut c_char = ptr::null_mut();
        // SAFETY: `profile` is a valid NUL-terminated string and `err` a valid
        // out pointer; flags 0 means the profile is a literal string.
        let rc = unsafe { sandbox_init(profile.as_ptr(), 0, &raw mut err) };
        if rc == 0 {
            return Ok(());
        }
        let msg = if err.is_null() {
            "sandbox_init failed".to_string()
        } else {
            // SAFETY: on failure `err` points at a NUL-terminated string owned
            // by the sandbox library, which is freed right after the copy.
            let s = unsafe { CStr::from_ptr(err) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: `err` was set by `sandbox_init` and is not used again.
            unsafe { sandbox_free_error(err) };
            s
        };
        Err(io::Error::other(msg))
    }
}
