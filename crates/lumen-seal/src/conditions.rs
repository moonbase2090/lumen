//! The Linux sealing conditions.
//!
//! Shell integration seals only if all of these hold, each read at the layer's
//! start (`ready`) and the system-wide ones read again at every command start
//! (`recheck`):
//!
//! 1. The terminal runs on Linux (macOS and every other system is grid-only).
//! 2. The shell is on the list and was run directly by its absolute path, with
//!    no wrapper and no `-c` command, and its `comm` is the listed name.
//! 3. Yama `ptrace_scope` is 1 or higher.
//! 4. `perf_event_paranoid` is 1 or higher.
//! 5. `dev.tty.legacy_tiocsti` exists and reads 0.
//! 6. The shell is in the terminal's own user namespace, which is the initial
//!    one (it has no parent).
//! 7. The shell's `uid_map` and `gid_map` are each the identity map, one line
//!    of the numbers 0, 0 and 4294967295.
//! 8. The shell's `CapInh`, `CapPrm`, `CapEff` and `CapAmb` all lack
//!    `CAP_SYS_PTRACE` (bit 19). `CapBnd` is not used.
//! 9. The terminal's pidfd for the shell is still unreadable after the reads.
//!
//! A value below the floor of 3 or 4 latches: every layer of the terminal,
//! including ones started later, is grid-only until the terminal exits, even
//! if the value rises again. A read that fails is a failed check but does not
//! latch, since it shows nothing about the value.

use crate::probe::Probe;
use std::fmt;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};

/// `CAP_SYS_PTRACE`.
const CAP_SYS_PTRACE_BIT: u32 = 19;

/// The kernel keeps 15 bytes of a process's `comm`.
const COMM_LEN: usize = 15;

/// The shell a layer would seal.
#[derive(Debug, Clone, Copy)]
pub struct Shell<'a> {
    /// The pid from the terminal's own spawn (never one read from elsewhere).
    pub pid: u32,
    /// The terminal's pidfd for that spawn.
    pub pidfd: Option<RawFd>,
    /// The absolute path the terminal executed.
    pub path: &'a str,
    /// Whether the terminal executed that path directly, with no wrapper, no
    /// `-c` command and no user program in between.
    pub direct: bool,
}

/// Why a layer is grid-only. Each names the check that failed, for the notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Not Linux.
    NotLinux,
    /// An earlier check in this terminal saw a value below its floor.
    Latched,
    /// The shell was not run directly by its path.
    NotDirect,
    /// The shell's path is not on the list.
    NotListed,
    /// `/proc/<pid>/comm` is not the listed name, or cannot be read.
    CommMismatch,
    /// Yama `ptrace_scope` is below 1.
    YamaTooLow(u32),
    /// Yama `ptrace_scope` cannot be read.
    YamaUnreadable,
    /// `perf_event_paranoid` is below 1.
    PerfParanoidTooLow(i64),
    /// `perf_event_paranoid` cannot be read.
    PerfParanoidUnreadable,
    /// `legacy_tiocsti` is not 0.
    TiocstiEnabled(u32),
    /// `legacy_tiocsti` does not exist or cannot be read.
    TiocstiUnreadable,
    /// The shell is not in the terminal's own initial user namespace.
    UserNamespace,
    /// The shell's `uid_map` or `gid_map` is not the identity map or cannot be read.
    IdMap(&'static str),
    /// A capability set holds `CAP_SYS_PTRACE`, or the sets cannot be read.
    Capabilities(&'static str),
    /// The terminal's pidfd for the shell is readable (the shell exited or was
    /// replaced), or there is none.
    ShellGone,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::NotLinux => write!(f, "shell integration is sealed on Linux only"),
            Reason::Latched => write!(
                f,
                "an earlier check saw an unsafe setting; grid-only until restart"
            ),
            Reason::NotDirect => write!(f, "the shell was not started directly"),
            Reason::NotListed => write!(f, "the shell is not on the list"),
            Reason::CommMismatch => write!(f, "the running program is not the listed shell"),
            Reason::YamaTooLow(v) => write!(f, "ptrace_scope is {v}, below 1"),
            Reason::YamaUnreadable => write!(f, "ptrace_scope cannot be read"),
            Reason::PerfParanoidTooLow(v) => write!(f, "perf_event_paranoid is {v}, below 1"),
            Reason::PerfParanoidUnreadable => write!(f, "perf_event_paranoid cannot be read"),
            Reason::TiocstiEnabled(v) => write!(f, "legacy_tiocsti is {v}, not 0"),
            Reason::TiocstiUnreadable => write!(f, "legacy_tiocsti is missing or cannot be read"),
            Reason::UserNamespace => write!(f, "the shell is not in the initial user namespace"),
            Reason::IdMap(which) => write!(f, "the shell's {which} is not the identity map"),
            Reason::Capabilities(which) => {
                write!(f, "{which} holds CAP_SYS_PTRACE or cannot be read")
            }
            Reason::ShellGone => write!(f, "the shell is gone"),
        }
    }
}

/// What a check decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Every condition holds: the layer may have a holder and keys.
    Seal,
    /// A condition failed: no holder, no keys, and a notice naming the check.
    GridOnly(Reason),
}

impl Decision {
    /// Whether the layer may be sealed.
    #[must_use]
    pub fn seals(&self) -> bool {
        matches!(self, Decision::Seal)
    }
}

/// The terminal-wide state of the checks: the latch. One per terminal.
#[derive(Debug, Default)]
pub struct Sealer {
    latched: AtomicBool,
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `text` is exactly one line of three whole numbers `0`, `0` and
/// `4294967295`, separated by runs of spaces or tabs. Parsed as numbers, so
/// `00` and `0x0` are not accidents of a string compare.
#[must_use]
pub fn is_identity_map(text: &str) -> bool {
    let mut lines = text.lines();
    let (Some(line), None) = (lines.next(), lines.next()) else {
        return false;
    };
    let numbers: Option<Vec<u64>> = line
        .split([' ', '\t'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.bytes().all(|b| b.is_ascii_digit()) {
                part.parse().ok()
            } else {
                None
            }
        })
        .collect();
    numbers.is_some_and(|n| n == [0, 0, u64::from(u32::MAX)])
}

/// The name of the first of `CapInh`, `CapPrm`, `CapEff` and `CapAmb` in a
/// `/proc/<pid>/status` that holds `CAP_SYS_PTRACE`, or that is missing or
/// unreadable. `None` when all four are present and lack the bit.
#[must_use]
pub fn capability_failure(status: &str) -> Option<&'static str> {
    for name in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|rest| u64::from_str_radix(rest.trim(), 16).ok());
        match value {
            Some(set) if set & (1 << CAP_SYS_PTRACE_BIT) == 0 => {}
            _ => return Some(name),
        }
    }
    None
}

impl Sealer {
    /// A sealer with the latch clear.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a check has latched.
    #[must_use]
    pub fn latched(&self) -> bool {
        self.latched.load(Ordering::SeqCst)
    }

    fn latch(&self) {
        self.latched.store(true, Ordering::SeqCst);
    }

    /// The checks at the layer's start, in full.
    pub fn ready(&self, probe: &dyn Probe, shell: &Shell<'_>, listed: &[&str]) -> Decision {
        if !probe.is_linux() {
            return Decision::GridOnly(Reason::NotLinux);
        }
        if self.latched() {
            return Decision::GridOnly(Reason::Latched);
        }
        if let Some(reason) = Self::identity(probe, shell, listed) {
            return Decision::GridOnly(reason);
        }
        if let Some(reason) = self.settings(probe) {
            return Decision::GridOnly(reason);
        }
        if let Some(reason) = Self::namespace(probe, shell) {
            return Decision::GridOnly(reason);
        }
        if probe.shell_alive(shell.pidfd) != Some(true) {
            return Decision::GridOnly(Reason::ShellGone);
        }
        Decision::Seal
    }

    /// The checks at every command start: the latch, the three system-wide
    /// settings and that the shell is still the process it was.
    pub fn recheck(&self, probe: &dyn Probe, shell: &Shell<'_>) -> Decision {
        if !probe.is_linux() {
            return Decision::GridOnly(Reason::NotLinux);
        }
        if self.latched() {
            return Decision::GridOnly(Reason::Latched);
        }
        if let Some(reason) = self.settings(probe) {
            return Decision::GridOnly(reason);
        }
        if probe.shell_alive(shell.pidfd) != Some(true) {
            return Decision::GridOnly(Reason::ShellGone);
        }
        Decision::Seal
    }

    fn identity(probe: &dyn Probe, shell: &Shell<'_>, listed: &[&str]) -> Option<Reason> {
        if !shell.direct {
            return Some(Reason::NotDirect);
        }
        if !listed.contains(&shell.path) || !shell.path.starts_with('/') {
            return Some(Reason::NotListed);
        }
        let want = basename(shell.path);
        let want = &want.as_bytes()[..want.len().min(COMM_LEN)];
        match probe.proc_text(shell.pid, "comm") {
            Some(comm) if comm.strip_suffix('\n').unwrap_or(&comm).as_bytes() == want => None,
            _ => Some(Reason::CommMismatch),
        }
    }

    fn settings(&self, probe: &dyn Probe) -> Option<Reason> {
        match probe.yama_scope() {
            None => return Some(Reason::YamaUnreadable),
            Some(v) if v < 1 => {
                self.latch();
                return Some(Reason::YamaTooLow(v));
            }
            Some(_) => {}
        }
        match probe.perf_paranoid() {
            None => return Some(Reason::PerfParanoidUnreadable),
            Some(v) if v < 1 => {
                self.latch();
                return Some(Reason::PerfParanoidTooLow(v));
            }
            Some(_) => {}
        }
        match probe.legacy_tiocsti() {
            None => Some(Reason::TiocstiUnreadable),
            Some(0) => None,
            Some(v) => Some(Reason::TiocstiEnabled(v)),
        }
    }

    fn namespace(probe: &dyn Probe, shell: &Shell<'_>) -> Option<Reason> {
        if probe.initial_user_namespace(shell.pid) != Some(true) {
            return Some(Reason::UserNamespace);
        }
        for (file, which) in [("uid_map", "uid_map"), ("gid_map", "gid_map")] {
            if !probe
                .proc_text(shell.pid, file)
                .is_some_and(|t| is_identity_map(&t))
            {
                return Some(Reason::IdMap(which));
            }
        }
        match probe.proc_text(shell.pid, "status") {
            Some(status) => capability_failure(&status).map(Reason::Capabilities),
            None => Some(Reason::Capabilities("status")),
        }
    }
}
