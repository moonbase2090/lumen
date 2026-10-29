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
    let started = start_with(holder(), &pidfd, shell.id(), std::process::id(), extra)
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
fn a_running_holder_has_no_new_privs_and_a_seccomp_filter() {
    let (mut s, mut shell, _pidfd) = launch(&[]);
    let pid = s.child.id();
    // No-new-privs is set just before the filter, so wait for both; a
    // container may put its own filter on every process, so the filter alone
    // shows nothing.
    wait_until("no-new-privs and the filter", || {
        status_field(pid, "NoNewPrivs").as_deref() == Some("1")
            && status_field(pid, "Seccomp").as_deref() == Some("2")
    });
    // Its descriptors are not read from /proc/<pid>/fd: it is not dumpable, so
    // the kernel refuses that to everyone but root. The check mode below has
    // it list its own.
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
    assert_eq!(
        fds, "1,3,4,5,6",
        "the seal channel, spawn socketpair, EOF pipe and pidfd, and the report's own standard output: {out}"
    );
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
fn a_holder_told_the_wrong_terminal_pid_does_not_run() {
    let (mut shell, pidfd) = stand_in_shell();
    let mut s = start_with(holder(), &pidfd, shell.id(), std::process::id() + 1, &[])
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

/// A holder for which this test process is the shell, so what the test sends
/// on the seal channel is what the shell sends. The report goes to standard output.
fn launch_with_this_process_as_the_shell() -> Started {
    let pidfd = pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("pidfd_open: {e}"));
    start_with(
        holder(),
        &pidfd,
        std::process::id(),
        std::process::id(),
        &["--report"],
    )
    .unwrap_or_else(|e| unreachable!("the holder starts: {e}"))
}

fn finish(started: Started) -> (Option<i32>, String) {
    let Started {
        mut child,
        seal,
        spawn: _spawn,
        eof_write: _eof,
    } = started;
    let stdout = child.stdout.take();
    drop(seal);
    let code = exit_code(&mut child);
    let mut out = String::new();
    if let Some(mut stdout) = stdout {
        let _ = stdout.read_to_string(&mut out);
    }
    (code, out)
}

fn send(fd: &OwnedFd, bytes: &[u8]) {
    let mut file = std::fs::File::from(fd.try_clone().unwrap_or_else(|e| unreachable!("{e}")));
    assert!(std::io::Write::write_all(&mut file, bytes).is_ok());
}

#[test]
fn a_ready_from_the_shell_is_accepted_once_and_a_second_is_refused() {
    let s = launch_with_this_process_as_the_shell();
    send(&s.seal, &lumen_holder::protocol::ready(1));
    send(&s.seal, &lumen_holder::protocol::ready(2));
    let (code, out) = finish(s);
    assert_eq!(code, Some(12), "{out}");
    assert!(
        out.contains("ready=1 second_ready=1 refused=0 malformed=0"),
        "{out}"
    );
}

#[test]
fn a_message_from_a_child_that_holds_the_descriptor_is_refused() {
    let s = launch_with_this_process_as_the_shell();
    // A child of the shell that inherited the channel writes a perfectly good ready.
    let child_end = s.seal.try_clone().unwrap_or_else(|e| unreachable!("{e}"));
    let status = Command::new("sh")
        .arg("-c")
        .arg("printf '\\001\\001\\000\\000\\000\\007'")
        .stdout(Stdio::from(child_end))
        .status();
    assert!(status.is_ok_and(|s| s.success()));
    let (code, out) = finish(s);
    assert_eq!(code, Some(12), "{out}");
    assert!(
        out.contains("ready=0 second_ready=0 refused=1 malformed=0"),
        "{out}"
    );
}

#[test]
fn a_refused_sender_does_not_use_up_the_shells_ready() {
    let s = launch_with_this_process_as_the_shell();
    let child_end = s.seal.try_clone().unwrap_or_else(|e| unreachable!("{e}"));
    let status = Command::new("sh")
        .arg("-c")
        .arg("printf '\\001\\001\\000\\000\\000\\007'")
        .stdout(Stdio::from(child_end))
        .status();
    assert!(status.is_ok_and(|s| s.success()));
    send(&s.seal, &lumen_holder::protocol::ready(1));
    let (code, out) = finish(s);
    assert_eq!(code, Some(12), "{out}");
    assert!(
        out.contains("ready=1 second_ready=0 refused=1 malformed=0"),
        "{out}"
    );
}

#[test]
fn malformed_messages_from_the_shell_are_dropped_and_counted_and_do_not_end_the_holder() {
    let s = launch_with_this_process_as_the_shell();
    let bad: [&[u8]; 6] = [
        &[2, 1, 0, 0, 0, 1],
        &[1, 9, 0, 0, 0, 1],
        &[1],
        &[1, 1, 0, 0, 0],
        &[1, 1, 0, 0, 0, 1, 0],
        &[1; 200],
    ];
    for m in bad {
        send(&s.seal, m);
    }
    send(&s.seal, &lumen_holder::protocol::ready(1));
    let (code, out) = finish(s);
    assert_eq!(code, Some(12), "{out}");
    assert!(
        out.contains("ready=1 second_ready=0 refused=0 malformed=6"),
        "{out}"
    );
}

#[test]
fn a_holder_whose_shell_start_time_was_wrong_refuses_even_the_real_shell() {
    // The pid is right and the user is right but the recorded start time is not
    // the process's: what a reused pid would look like.
    let pidfd = pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("{e}"));
    let s = start_with(
        holder(),
        &pidfd,
        std::process::id(),
        std::process::id(),
        &["--report", "--shell-start", "1"],
    );
    // A later --shell-start wins in the holder's argument loop.
    let s = s.unwrap_or_else(|e| unreachable!("{e}"));
    send(&s.seal, &lumen_holder::protocol::ready(1));
    let (code, out) = finish(s);
    assert_eq!(code, Some(12), "{out}");
    assert!(
        out.contains("ready=0 second_ready=0 refused=1 malformed=0"),
        "{out}"
    );
}
