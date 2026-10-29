//! Who may use the seal channel.
//!
//! The descriptor is not the authority: any process that inherited it can
//! write to it. The principal is the interactive shell process itself, named
//! by its pid and the time it started, both taken by the terminal at spawn
//! while its pidfd pinned the pid. A message is accepted only if the kernel
//! says, in the credentials it attached to that message, that it was sent by
//! that pid, as the same user, and the process with that pid still has the
//! recorded start time (a reused pid does not).
//!
//! `SO_PASSCRED` is what makes the kernel attach those credentials. The terminal
//! sets it on the holder's end of the socketpair before it starts the holder,
//! so it is already set when the shell is handed its end; the holder sets it
//! again when it starts, which changes nothing. A `ready` therefore cannot be
//! lost to a missing option. If a message does arrive without credentials, for
//! any reason, it is refused and counted, nothing is sent back, and the layer
//! stays not ready: the shell sees no new command blocks, and the layer is
//! grid-only once the ready timeout (not built yet) runs out.

use std::path::Path;

/// The sender the kernel recorded for a message (`SCM_CREDENTIALS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Creds {
    /// The sending process.
    pub pid: i32,
    /// Its user.
    pub uid: u32,
    /// Its group.
    pub gid: u32,
}

/// The process that may seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Principal {
    /// The shell's pid.
    pub pid: u32,
    /// Its start time in clock ticks since boot, `/proc/<pid>/stat` field 22.
    pub start_time: u64,
    /// The user the holder runs as, which the shell must also be.
    pub uid: u32,
}

/// What was decided about a message's sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The shell itself.
    Accept,
    /// The kernel attached no credentials.
    NoCredentials,
    /// Another process (an exec'd child, a subshell, a pipeline stage), or one
    /// the holder cannot see by pid.
    WrongProcess,
    /// The right pid under another user.
    WrongUser,
    /// The pid is the shell's but the process behind it started at another time.
    StartTimeChanged,
    /// The start time of the pid could not be read.
    StartTimeUnreadable,
}

/// Judge a message's sender. `start_time_of` reads a pid's start time.
#[must_use]
pub fn judge(
    principal: &Principal,
    creds: Option<Creds>,
    start_time_of: &dyn Fn(u32) -> Option<u64>,
) -> Verdict {
    let Some(creds) = creds else {
        return Verdict::NoCredentials;
    };
    if u32::try_from(creds.pid).ok() != Some(principal.pid) {
        return Verdict::WrongProcess;
    }
    if creds.uid != principal.uid {
        return Verdict::WrongUser;
    }
    match start_time_of(principal.pid) {
        None => Verdict::StartTimeUnreadable,
        Some(t) if t == principal.start_time => Verdict::Accept,
        Some(_) => Verdict::StartTimeChanged,
    }
}

/// Field 22 of a `/proc/<pid>/stat`, the start time. The command name is the
/// second field, in parentheses, and may hold spaces and parentheses itself,
/// so the fields are counted from the last `)`.
#[must_use]
pub fn parse_start_time(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    // After the command: state is field 3, so start time (22) is the 20th.
    after.split_ascii_whitespace().nth(19)?.parse().ok()
}

/// The start time of `pid`, read under `proc` (normally `/proc`).
#[must_use]
pub fn start_time_in(proc: &Path, pid: u32) -> Option<u64> {
    parse_start_time(&std::fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?)
}

/// The start time of `pid` on this system.
#[must_use]
pub fn start_time(pid: u32) -> Option<u64> {
    start_time_in(Path::new("/proc"), pid)
}
