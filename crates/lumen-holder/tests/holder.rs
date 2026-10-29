//! The holder as a real process on Linux: what it is given, what it refuses to
//! do, and what ends it.

#![cfg(target_os = "linux")]

use lumen_holder::ffi_linux::{die_with_parent, dumpable, pidfd_open, set_dumpable};
use lumen_holder::spawn::{Started, start_with};
use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn holder() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_lumen-holder"))
}

/// A stand-in for the shell: a process that sleeps, and the terminal's pidfd for it.
fn stand_in_shell() -> (Child, OwnedFd) {
    let child = Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("sleep starts: {e}"));
    let pidfd = pidfd_open(child.id()).unwrap_or_else(|e| unreachable!("pidfd_open: {e}"));
    (child, pidfd)
}

fn launch(extra: &[&str]) -> (Started, Child, OwnedFd) {
    let (shell, pidfd) = stand_in_shell();
    let started = start_with(holder(), &pidfd, std::process::id(), extra)
        .unwrap_or_else(|e| unreachable!("the holder starts: {e}"));
    (started, shell, pidfd)
}

fn status_field(pid: u32, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
        .map(|v| v.trim().to_owned())
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    unreachable!("timed out waiting for {what}");
}

fn exit_code(child: &mut Child) -> Option<i32> {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    None
}

#[test]
fn a_running_holder_has_four_descriptors_no_new_privs_and_a_seccomp_filter() {
    let (mut s, mut shell, _pidfd) = launch(&[]);
    let pid = s.child.id();
    // No-new-privs is set just before the filter, so wait for both; a
    // container may put its own filter on every process, so the filter alone
    // shows nothing.
    wait_until("no-new-privs and the filter", || {
        status_field(pid, "NoNewPrivs").as_deref() == Some("1")
            && status_field(pid, "Seccomp").as_deref() == Some("2")
    });
    let mut fds: Vec<u32> = std::fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap_or_else(|e| unreachable!("the holder's fds are listed: {e}"))
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .collect();
    fds.sort_unstable();
    assert_eq!(
        fds,
        [3, 4, 5, 6],
        "exactly the seal channel, spawn socketpair, EOF pipe and pidfd"
    );
    assert!(
        s.child.try_wait().is_ok_and(|w| w.is_none()),
        "and it is still running"
    );
    let _ = s.child.kill();
    let _ = shell.kill();
}

#[test]
fn the_hardened_process_cannot_fork_exec_or_start_a_thread_and_is_not_dumpable() {
    let (mut s, mut shell, _pidfd) = launch(&["--check-hardening"]);
    let mut out = String::new();
    if let Some(mut stdout) = s.child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    assert_eq!(exit_code(&mut s.child), Some(0), "{out}");
    assert!(out.contains("dumpable=Some(false)"), "{out}");
    assert!(out.contains("spawn=refused (PermissionDenied)"), "{out}");
    assert!(
        out.contains("exec=refused (PermissionDenied)"),
        "an exec in place is refused too: {out}"
    );
    assert!(out.contains("thread=refused (PermissionDenied)"), "{out}");
    let fds = out
        .lines()
        .find_map(|l| l.strip_prefix("fds="))
        .unwrap_or("");
    let fds: Vec<&str> = fds.split(',').collect();
    for wanted in ["1", "3", "4", "5", "6"] {
        assert!(fds.contains(&wanted), "{out}");
    }
    for closed in ["0", "2"] {
        assert!(
            !fds.contains(&closed),
            "descriptor {closed} was closed: {out}"
        );
    }
    let _ = shell.kill();
}

#[test]
fn the_terminal_closing_the_eof_pipe_ends_the_holder() {
    let (s, mut shell, _pidfd) = launch(&[]);
    let Started {
        mut child,
        eof_write,
        seal: _seal,
        spawn: _spawn,
    } = s;
    drop(eof_write);
    assert_eq!(exit_code(&mut child), Some(10));
    let _ = shell.kill();
}

#[test]
fn the_shell_exiting_ends_the_holder() {
    let (s, mut shell, _pidfd) = launch(&[]);
    let Started {
        mut child,
        seal: _seal,
        spawn: _spawn,
        eof_write: _eof,
    } = s;
    let _ = shell.kill();
    let _ = shell.wait();
    assert_eq!(exit_code(&mut child), Some(11));
}

#[test]
fn the_seal_channel_closing_ends_the_holder() {
    let (s, mut shell, _pidfd) = launch(&[]);
    let Started {
        mut child,
        seal,
        spawn: _spawn,
        eof_write: _eof,
    } = s;
    drop(seal);
    assert_eq!(exit_code(&mut child), Some(12));
    let _ = shell.kill();
}

#[test]
fn a_message_on_the_seal_channel_ends_the_holder_since_no_request_is_defined() {
    let (s, mut shell, _pidfd) = launch(&[]);
    let Started {
        mut child,
        seal,
        spawn: _spawn,
        eof_write: _eof,
    } = s;
    let mut file = std::fs::File::from(seal);
    assert!(std::io::Write::write_all(&mut file, b"x").is_ok());
    assert_eq!(exit_code(&mut child), Some(13));
    let _ = shell.kill();
}

#[test]
fn a_holder_told_the_wrong_terminal_pid_does_not_run() {
    let (mut shell, pidfd) = stand_in_shell();
    let mut s = start_with(holder(), &pidfd, std::process::id() + 1, &[])
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        exit_code(&mut s.child),
        Some(70),
        "its parent is not who it was told"
    );
    let _ = shell.kill();
}

#[test]
fn die_with_parent_refuses_a_parent_that_is_not_the_parent() {
    let e = die_with_parent(std::process::id() + 1);
    assert!(e.is_err());
}

#[test]
fn dumpable_can_be_turned_off_and_read_back() {
    assert!(set_dumpable(false).is_ok());
    assert_eq!(dumpable().ok(), Some(false));
}

#[test]
fn the_spawn_leaves_the_terminals_ends_close_on_exec() {
    let (s, mut shell, _pidfd) = launch(&[]);
    for (name, fd) in [
        ("seal", &s.seal),
        ("spawn", &s.spawn),
        ("eof", &s.eof_write),
    ] {
        use std::os::fd::AsRawFd;
        let flags = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))
            .unwrap_or_default();
        let flags = flags
            .lines()
            .find_map(|l| l.strip_prefix("flags:"))
            .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
            .unwrap_or(0);
        assert_ne!(flags & 0o2_000_000, 0, "{name} is close-on-exec");
    }
    let mut s = s;
    let _ = s.child.kill();
    let _ = shell.kill();
}
