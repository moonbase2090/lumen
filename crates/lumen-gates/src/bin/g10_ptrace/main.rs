//! Gate G10: same-uid process-inspection results across the Linux Yama matrix.
//!
//! Run this binary once per Yama scope as an ordinary user. It exercises an
//! attacker that is the target's parent, sibling and child against ptrace
//! attach, `/proc/<pid>/mem`, `process_vm_readv` and `pidfd_getfd`. The helper
//! script `scripts/g10-yama-matrix.sh` drives scopes 0 through 3 in order on a
//! disposable Linux VM. Scope 3 cannot be lowered until reboot.

#![deny(unsafe_code)]

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "named FFI module for ptrace and process-inspection syscalls; see its safety comments"
)]
mod ffi_linux;

use std::fmt;
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relationship {
    ParentToChild,
    SiblingToSibling,
    ChildToParent,
}

impl Relationship {
    const ALL: [Self; 3] = [
        Self::ParentToChild,
        Self::SiblingToSibling,
        Self::ChildToParent,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::ParentToChild => "parent-to-child",
            Self::SiblingToSibling => "sibling-to-sibling",
            Self::ChildToParent => "child-to-parent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    PtraceAttach,
    ProcMem,
    ProcessVmReadv,
    PidfdGetfd,
}

impl Operation {
    const ALL: [Self; 4] = [
        Self::PtraceAttach,
        Self::ProcMem,
        Self::ProcessVmReadv,
        Self::PidfdGetfd,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::PtraceAttach => "ptrace",
            Self::ProcMem => "proc-mem",
            Self::ProcessVmReadv => "process-vm-readv",
            Self::PidfdGetfd => "pidfd-getfd",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Allowed,
    Denied(i32),
    Unsupported,
    WrongValue,
}

impl Outcome {
    fn from_error(error: &std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(libc::ENOSYS) => Self::Unsupported,
            Some(errno) => Self::Denied(errno),
            None => Self::Denied(-1),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        if text == "allowed" {
            return Some(Self::Allowed);
        }
        if text == "unsupported" {
            return Some(Self::Unsupported);
        }
        if text == "wrong-value" {
            return Some(Self::WrongValue);
        }
        Some(Self::Denied(text.strip_prefix("denied:")?.parse().ok()?))
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allowed => f.write_str("allowed"),
            Self::Denied(errno) => write!(f, "denied:{errno}"),
            Self::Unsupported => f.write_str("unsupported"),
            Self::WrongValue => f.write_str("wrong-value"),
        }
    }
}

fn expected_allowed(scope: u32, relationship: Relationship) -> bool {
    match scope {
        0 => true,
        1 => relationship == Relationship::ParentToChild,
        2 | 3 => false,
        _ => false,
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ExitCode, ffi_linux};
    use super::{Operation, Outcome, Relationship, expected_allowed};
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, RawFd};
    use std::process::{Child, ChildStdin, Command, Stdio};

    const SECRET: u64 = 0x6c75_6d65_6e31_3047;
    static TARGET_VALUE: u64 = SECRET;

    struct Target {
        child: Child,
        input: Option<ChildStdin>,
        pid: u32,
        address: usize,
        fd: RawFd,
    }

    impl Drop for Target {
        fn drop(&mut self) {
            self.input.take();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn info() -> Option<(u32, usize, File)> {
        let file = File::open("/dev/null").ok()?;
        let address = (&raw const TARGET_VALUE) as usize;
        Some((std::process::id(), address, file))
    }

    fn target_wait() -> ExitCode {
        let Some((pid, address, file)) = info() else {
            return ExitCode::FAILURE;
        };
        println!("{pid} {address} {}", file.as_raw_fd());
        let _ = std::io::stdout().flush();
        let mut byte = [0u8; 1];
        let _ = std::io::stdin().read(&mut byte);
        ExitCode::SUCCESS
    }

    fn parse_info(text: &str) -> Option<(u32, usize, RawFd)> {
        let mut fields = text.split_whitespace();
        let result = Some((
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
        ));
        if fields.next().is_some() {
            return None;
        }
        result
    }

    fn spawn_target() -> Option<Target> {
        let exe = std::env::current_exe().ok()?;
        let mut child = Command::new(exe)
            .arg("target-wait")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut output = BufReader::new(child.stdout.take()?);
        let mut line = String::new();
        output.read_line(&mut line).ok()?;
        let (pid, address, fd) = parse_info(&line)?;
        Some(Target {
            input: child.stdin.take(),
            child,
            pid,
            address,
            fd,
        })
    }

    fn proc_mem(pid: u32, address: usize) -> std::io::Result<u64> {
        let mut file = File::open(format!("/proc/{pid}/mem"))?;
        file.seek(SeekFrom::Start(address as u64))?;
        let mut bytes = [0u8; std::mem::size_of::<u64>()];
        file.read_exact(&mut bytes)?;
        Ok(u64::from_ne_bytes(bytes))
    }

    fn attempt(operation: Operation, pid: u32, address: usize, fd: RawFd) -> Outcome {
        let result = match operation {
            Operation::PtraceAttach => ffi_linux::ptrace_round_trip(pid).map(|()| SECRET),
            Operation::ProcMem => proc_mem(pid, address),
            Operation::ProcessVmReadv => ffi_linux::process_vm_read(pid, address),
            Operation::PidfdGetfd => ffi_linux::pidfd_open(pid)
                .and_then(|pidfd| ffi_linux::pidfd_getfd(pidfd.as_raw_fd(), fd).map(|_| SECRET)),
        };
        match result {
            Ok(value) if value == SECRET => Outcome::Allowed,
            Ok(_) => Outcome::WrongValue,
            Err(error) => Outcome::from_error(&error),
        }
    }

    fn attack(pid: u32, address: usize, fd: RawFd) -> Vec<Outcome> {
        Operation::ALL
            .into_iter()
            .map(|operation| attempt(operation, pid, address, fd))
            .collect()
    }

    fn print_outcomes(outcomes: &[Outcome]) {
        for (operation, outcome) in Operation::ALL.into_iter().zip(outcomes) {
            print!("{}={} ", operation.name(), outcome);
        }
        println!();
    }

    fn parse_outcomes(text: &str) -> Option<Vec<Outcome>> {
        let mut outcomes = Vec::new();
        let mut fields = text.split_whitespace();
        for operation in Operation::ALL {
            let field = fields.next()?;
            let (name, value) = field.split_once('=')?;
            if name != operation.name() {
                return None;
            }
            outcomes.push(Outcome::parse(value)?);
        }
        (fields.next().is_none()).then_some(outcomes)
    }

    fn attacker(arguments: &[String]) -> ExitCode {
        let [pid, address, fd] = arguments else {
            return ExitCode::FAILURE;
        };
        let (Ok(pid), Ok(address), Ok(fd)) = (pid.parse(), address.parse(), fd.parse()) else {
            return ExitCode::FAILURE;
        };
        print_outcomes(&attack(pid, address, fd));
        ExitCode::SUCCESS
    }

    fn attacker_command(pid: u32, address: usize, fd: RawFd) -> Option<Vec<Outcome>> {
        let output = Command::new(std::env::current_exe().ok()?)
            .args([
                "attacker",
                &pid.to_string(),
                &address.to_string(),
                &fd.to_string(),
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_outcomes(std::str::from_utf8(&output.stdout).ok()?)
    }

    fn child_to_parent_target() -> ExitCode {
        let Some((pid, address, file)) = info() else {
            return ExitCode::FAILURE;
        };
        let Some(outcomes) = attacker_command(pid, address, file.as_raw_fd()) else {
            return ExitCode::FAILURE;
        };
        print_outcomes(&outcomes);
        ExitCode::SUCCESS
    }

    fn run_relationship(relationship: Relationship) -> Option<Vec<Outcome>> {
        match relationship {
            Relationship::ParentToChild => {
                let target = spawn_target()?;
                Some(attack(target.pid, target.address, target.fd))
            }
            Relationship::SiblingToSibling => {
                let target = spawn_target()?;
                attacker_command(target.pid, target.address, target.fd)
            }
            Relationship::ChildToParent => {
                let output = Command::new(std::env::current_exe().ok()?)
                    .arg("child-to-parent-target")
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                parse_outcomes(std::str::from_utf8(&output.stdout).ok()?)
            }
        }
    }

    fn yama_scope() -> Option<u32> {
        std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    fn has_ptrace_capability() -> bool {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    let value = line.strip_prefix("CapEff:")?.trim();
                    u64::from_str_radix(value, 16).ok()
                })
            })
            .is_none_or(|set| set & (1 << 19) != 0)
    }

    pub fn run() -> ExitCode {
        let Some(scope @ 0..=3) = yama_scope() else {
            println!("G10 FAIL ptrace_scope is unreadable or outside 0..=3");
            return ExitCode::FAILURE;
        };
        if lumen_holder::ffi_linux::effective_uid() == 0 || has_ptrace_capability() {
            println!("G10 FAIL run as an unprivileged user without CAP_SYS_PTRACE");
            return ExitCode::FAILURE;
        }
        let mut all = true;
        println!("G10 scope={scope}");
        for relationship in Relationship::ALL {
            let outcomes = run_relationship(relationship);
            let expected = expected_allowed(scope, relationship);
            let passed = outcomes.as_ref().is_some_and(|results| {
                results.iter().all(|result| match result {
                    Outcome::Allowed => expected,
                    Outcome::Denied(errno) => {
                        !expected && matches!(*errno, libc::EPERM | libc::EACCES)
                    }
                    Outcome::Unsupported | Outcome::WrongValue => false,
                })
            });
            all &= passed;
            let detail = outcomes.map_or_else(
                || "not-run".to_owned(),
                |results| {
                    Operation::ALL
                        .into_iter()
                        .zip(results)
                        .map(|(operation, outcome)| format!("{}={outcome}", operation.name()))
                        .collect::<Vec<_>>()
                        .join(" ")
                },
            );
            println!(
                "G10 {} {} expected={} {detail}",
                if passed { "ok" } else { "FAIL" },
                relationship.name(),
                if expected { "allowed" } else { "denied" }
            );
        }
        println!("G10 {} scope={scope}", if all { "PASS" } else { "FAIL" });
        if all {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    pub fn role(arguments: &[String]) -> Option<ExitCode> {
        match arguments {
            [role] if role == "target-wait" => Some(target_wait()),
            [role] if role == "child-to-parent-target" => Some(child_to_parent_target()),
            [role, rest @ ..] if role == "attacker" => Some(attacker(rest)),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        linux::role(&arguments).unwrap_or_else(linux::run)
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!("G10 SKIP unsupported platform");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_yama_relationship_matrix_is_explicit_for_every_scope() {
        for scope in 0..=3 {
            for relationship in Relationship::ALL {
                assert_eq!(
                    expected_allowed(scope, relationship),
                    scope == 0 || scope == 1 && relationship == Relationship::ParentToChild
                );
            }
        }
    }

    #[test]
    fn outcome_text_round_trips_without_conflating_unsupported_and_denied() {
        for outcome in [
            Outcome::Allowed,
            Outcome::Denied(libc::EPERM),
            Outcome::Unsupported,
            Outcome::WrongValue,
        ] {
            assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
        }
    }
}
