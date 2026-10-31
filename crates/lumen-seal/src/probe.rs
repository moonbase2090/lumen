//! What the system says, as the sealing conditions ask for it.
//!
//! Every method returns `None` when the answer cannot be had (a missing file,
//! a read error, a platform without the call). The conditions treat that as a
//! failed check, so nothing here ever defaults to "fine".

use std::os::fd::RawFd;
use std::path::PathBuf;

/// The facts the sealing conditions are decided on.
pub trait Probe {
    /// Whether this is Linux. Anywhere else the layer is grid-only.
    fn is_linux(&self) -> bool;
    /// `kernel.yama.ptrace_scope`.
    fn yama_scope(&self) -> Option<u32>;
    /// `dev.tty.legacy_tiocsti`.
    fn legacy_tiocsti(&self) -> Option<u32>;
    /// `kernel.perf_event_paranoid`, which can be negative.
    fn perf_paranoid(&self) -> Option<i64>;
    /// The text of `/proc/<pid>/<name>` (`comm`, `cmdline`, `stat`, `status`,
    /// `uid_map`, `gid_map`). A non-UTF-8 value is unreadable and fails closed.
    fn proc_text(&self, pid: u32, name: &str) -> Option<String>;
    /// Whether `/proc/<pid>/ns/user` is the same namespace as the terminal's
    /// own, and has no parent namespace (the initial user namespace).
    fn initial_user_namespace(&self, pid: u32) -> Option<bool>;
    /// Whether the shell is still the process it was, by the terminal's own
    /// pidfd for it: the descriptor is still unreadable. `None` when there is
    /// no pidfd to ask.
    fn shell_alive(&self, pidfd: Option<RawFd>) -> Option<bool>;
}

/// The probe that reads a Linux system.
///
/// The roots are parameters so that tests can point them at a directory they
/// built; the defaults are `/proc` and `/proc/sys`.
#[derive(Debug, Clone)]
pub struct FsProbe {
    proc: PathBuf,
    sys: PathBuf,
}

impl Default for FsProbe {
    fn default() -> Self {
        Self::at("/proc", "/proc/sys")
    }
}

impl FsProbe {
    /// A probe reading `proc` (as `/proc`) and `sys` (as `/proc/sys`).
    #[must_use]
    pub fn at(proc: impl Into<PathBuf>, sys: impl Into<PathBuf>) -> Self {
        Self {
            proc: proc.into(),
            sys: sys.into(),
        }
    }

    fn sysctl(&self, name: &str) -> Option<String> {
        let path = name
            .split('.')
            .fold(self.sys.clone(), |p, part| p.join(part));
        std::fs::read_to_string(path).ok()
    }
}

/// A sysctl value: one integer and a line end, nothing else.
fn integer(text: &str) -> Option<i64> {
    text.strip_suffix('\n').unwrap_or(text).parse().ok()
}

impl Probe for FsProbe {
    fn is_linux(&self) -> bool {
        cfg!(target_os = "linux")
    }

    fn yama_scope(&self) -> Option<u32> {
        u32::try_from(integer(&self.sysctl("kernel.yama.ptrace_scope")?)?).ok()
    }

    fn legacy_tiocsti(&self) -> Option<u32> {
        u32::try_from(integer(&self.sysctl("dev.tty.legacy_tiocsti")?)?).ok()
    }

    fn perf_paranoid(&self) -> Option<i64> {
        integer(&self.sysctl("kernel.perf_event_paranoid")?)
    }

    fn proc_text(&self, pid: u32, name: &str) -> Option<String> {
        if name.contains('/') || name.contains("..") {
            return None;
        }
        std::fs::read_to_string(self.proc.join(pid.to_string()).join(name)).ok()
    }

    #[cfg(target_os = "linux")]
    fn initial_user_namespace(&self, pid: u32) -> Option<bool> {
        let theirs = std::fs::read_link(self.proc.join(pid.to_string()).join("ns/user")).ok()?;
        let ours = std::fs::read_link(self.proc.join("self/ns/user")).ok()?;
        if theirs != ours {
            return Some(false);
        }
        crate::ffi_linux::user_namespace_has_no_parent(
            &self.proc.join(pid.to_string()).join("ns/user"),
        )
    }

    #[cfg(not(target_os = "linux"))]
    fn initial_user_namespace(&self, _pid: u32) -> Option<bool> {
        None
    }

    #[cfg(target_os = "linux")]
    fn shell_alive(&self, pidfd: Option<RawFd>) -> Option<bool> {
        crate::ffi_linux::pidfd_is_unreadable(pidfd?)
    }

    #[cfg(not(target_os = "linux"))]
    fn shell_alive(&self, _pidfd: Option<RawFd>) -> Option<bool> {
        None
    }
}
