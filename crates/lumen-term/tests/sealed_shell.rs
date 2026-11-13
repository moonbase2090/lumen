//! The sealed shell on Linux, end to end: the real terminal session starts a
//! real zsh with its end of the seal channel and the shim, the sealing
//! conditions decide, the real holder runs, and one command produces sealed
//! frames that reach the document as authenticated events.
//!
//! The system settings the conditions read are given by a probe that fakes only
//! the three sysctls, so the result does not depend on the machine running the
//! test; everything about the shell process is read for real.

#![cfg(target_os = "linux")]

use lumen_seal::{FsProbe, Probe, Reason, Sealer};
use lumen_sealed::types::{KIND_COMMAND_END, KIND_COMMAND_START, KIND_PROMPT};
use lumen_term::seal::{Config, LISTED, Status};
use lumen_term::{Event, Layer, Session};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Fakes `yama`, `perf_event_paranoid` and `legacy_tiocsti`; reads the rest.
struct Sysctls {
    yama: u32,
    real: FsProbe,
}

impl Probe for Sysctls {
    fn is_linux(&self) -> bool {
        self.real.is_linux()
    }
    fn yama_scope(&self) -> Option<u32> {
        Some(self.yama)
    }
    fn legacy_tiocsti(&self) -> Option<u32> {
        Some(0)
    }
    fn perf_paranoid(&self) -> Option<i64> {
        Some(2)
    }
    fn proc_text(&self, pid: u32, name: &str) -> Option<String> {
        self.real.proc_text(pid, name)
    }
    fn initial_user_namespace(&self, pid: u32) -> Option<bool> {
        self.real.initial_user_namespace(pid)
    }
    fn shell_alive(&self, pidfd: Option<lumen_seal::PidFd>) -> Option<bool> {
        self.real.shell_alive(pidfd)
    }
}

fn zsh() -> Option<String> {
    LISTED
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .map(|p| (*p).to_owned())
}

/// The holder executable, from the build itself: cargo says where it put it, so
/// no layout of the target directory is assumed. Asked once, because the tests
/// run in parallel and must not build it under each other; a build with nothing
/// to do is quick.
fn holder() -> PathBuf {
    static HOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOLDER
        .get_or_init(|| {
            let out = std::process::Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-p",
                    "lumen-holder",
                    "--features",
                    "seal-requests",
                    "--message-format=json",
                ])
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap_or_else(|e| unreachable!("cargo runs: {e}"));
            assert!(out.status.success(), "the holder is built");
            let text = String::from_utf8_lossy(&out.stdout);
            let path = text
                .lines()
                .filter(|l| l.contains("\"target\":{") && l.contains("\"name\":\"lumen-holder\""))
                .filter_map(|l| l.split("\"executable\":\"").nth(1))
                .filter_map(|r| r.split('"').next())
                .next_back()
                .map(PathBuf::from)
                .unwrap_or_default();
            assert!(path.is_file(), "the holder is at {}", path.display());
            path
        })
        .clone()
}

fn config(yama: u32, home: &std::path::Path) -> Config {
    let mut environment = BTreeMap::new();
    // A home with an empty rc file, so the shell does not stop at its first-run
    // questions and the user's own files do not matter.
    environment.insert("HOME".to_owned(), home.display().to_string());
    environment.insert("ZDOTDIR".to_owned(), home.display().to_string());
    environment.insert("PS1".to_owned(), "% ".to_owned());
    Config {
        holder: holder(),
        listed: LISTED.iter().map(|s| (*s).to_owned()).collect(),
        probe: Box::new(Sysctls {
            yama,
            real: FsProbe::default(),
        }),
        sealer: Arc::new(Sealer::new()),
        environment,
    }
}

fn home(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    assert!(std::fs::write(dir.join(".zshrc"), "PS1='% '\n").is_ok());
    dir
}

fn layer() -> Box<dyn Layer + Send> {
    Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }))
}

/// A zsh to test with, or a reason to skip. CI has one, and a missing one there
/// is a failure.
fn shell_or_skip() -> Option<String> {
    let found = zsh();
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "zsh is not installed on this CI machine"
        );
        eprintln!("skipped: no listed zsh on this machine");
    }
    found
}

/// Pump the session until `done` or the time is up, keeping the events.
fn pump_until(
    s: &mut Session,
    events: &mut Vec<Event>,
    done: impl Fn(&Session, &[Event]) -> bool,
) -> bool {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        s.pump_wait(Duration::from_millis(100));
        events.extend(s.take_events());
        if done(s, events) {
            return true;
        }
    }
    false
}

fn sealed(events: &[Event]) -> Vec<(u8, Vec<u8>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Sealed { kind, body } => Some((*kind, body.clone())),
            _ => None,
        })
        .collect()
}

fn text(s: &Session) -> String {
    s.document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
            lumen_term::Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn one_command_in_a_real_zsh_produces_sealed_frames_from_a_real_holder() {
    let Some(shell) = shell_or_skip() else { return };
    let cfg = config(1, &home("sealed-shell-ok"));
    let mut s = Session::start_with_seal(layer(), 100, 30, &shell, &[] as &[&str], &cfg)
        .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    assert_eq!(s.seal_status(), Status::Sealed);
    let holder_pid = s
        .holder_pid()
        .unwrap_or_else(|| unreachable!("a holder runs"));
    let comm = std::fs::read_to_string(format!("/proc/{holder_pid}/comm")).unwrap_or_default();
    assert_eq!(comm.trim(), "lumen-holder");

    let mut events = Vec::new();
    // The first prompt is sealed: the shim said ready and asked for it.
    assert!(
        pump_until(&mut s, &mut events, |_, e| sealed(e)
            .iter()
            .any(|(k, _)| *k == KIND_PROMPT)),
        "no sealed prompt arrived: {:?}",
        text(&s)
    );
    events.clear();
    assert!(s.send(b"true\n").is_ok());
    assert!(
        pump_until(&mut s, &mut events, |_, e| sealed(e)
            .iter()
            .filter(|(k, _)| *k == KIND_PROMPT)
            .count()
            >= 1
            && sealed(e).iter().any(|(k, _)| *k == KIND_COMMAND_END)),
        "one command did not give start, end and prompt: {:?} {:?}",
        sealed(&events),
        text(&s)
    );
    let got = sealed(&events);
    let kinds: Vec<u8> = got.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, [KIND_COMMAND_START, KIND_COMMAND_END, KIND_PROMPT]);
    assert_eq!(
        got[0].1,
        1u32.to_be_bytes().to_vec(),
        "the first command is number 1"
    );
    let mut end = 1u32.to_be_bytes().to_vec();
    end.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(got[1].1, end, "command 1 ended with status 0");
    assert_eq!(got[2].1, Vec::<u8>::new());

    // A failing command carries its status.
    events.clear();
    assert!(s.send(b"false\n").is_ok());
    assert!(pump_until(&mut s, &mut events, |_, e| sealed(e)
        .iter()
        .any(|(k, _)| *k == KIND_COMMAND_END)));
    let got = sealed(&events);
    let mut end = 2u32.to_be_bytes().to_vec();
    end.extend_from_slice(&1u32.to_be_bytes());
    assert!(
        got.iter().any(|(k, b)| *k == KIND_COMMAND_END && *b == end),
        "{got:?}"
    );
}

#[test]
fn a_shell_started_inside_the_sealed_shell_has_no_seal_channel() {
    let Some(shell) = shell_or_skip() else { return };
    let cfg = config(1, &home("sealed-shell-nested"));
    let mut s = Session::start_with_seal(layer(), 100, 30, &shell, &[] as &[&str], &cfg)
        .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    assert_eq!(s.seal_status(), Status::Sealed);
    let mut events = Vec::new();
    assert!(pump_until(&mut s, &mut events, |_, e| !sealed(e).is_empty()));
    // The count of sockets a child holds, and whether the shim's variables are gone.
    assert!(
        s.send(b"echo \"sockets:$(ls -l /proc/self/fd | grep -c socket) zdotdir:${ZDOTDIR:+set}${LUMEN_USER_ZDOTDIR:+leak}\"\n")
            .is_ok()
    );
    assert!(
        pump_until(&mut s, &mut events, |s, _| text(s).contains("sockets:")),
        "{:?}",
        text(&s)
    );
    let shown = text(&s);
    assert!(
        shown.contains("sockets:0"),
        "a child holds the seal channel: {shown:?}"
    );
    assert!(
        !shown.contains("leak"),
        "the shim's variable reached a child: {shown:?}"
    );
}

#[test]
fn a_failed_condition_starts_no_holder_and_the_shell_still_runs() {
    let Some(shell) = shell_or_skip() else { return };
    // Yama at 0: below the floor, so the layer is grid-only and the check latches.
    let cfg = config(0, &home("sealed-shell-grid"));
    let mut s = Session::start_with_seal(layer(), 100, 30, &shell, &[] as &[&str], &cfg)
        .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    assert_eq!(
        s.seal_status(),
        Status::GridOnly(Reason::YamaTooLow(0).to_string())
    );
    assert!(s.holder_pid().is_none(), "no holder for a grid-only layer");
    let mut events = Vec::new();
    assert!(s.send(b"echo grid-only-ok\n").is_ok());
    assert!(
        pump_until(&mut s, &mut events, |s, _| text(s).contains("grid-only-ok")),
        "{:?}",
        text(&s)
    );
    assert!(
        sealed(&events).is_empty(),
        "nothing sealed without keys: {:?}",
        sealed(&events)
    );
    assert!(cfg.sealer.latched(), "a value below the floor latches");
}

#[test]
fn a_program_that_is_not_a_listed_shell_has_no_sealed_layer() {
    let cfg = config(1, &home("sealed-shell-na"));
    let mut s = Session::start_with_seal(layer(), 80, 24, "/bin/sh", &["-c", "echo hi"], &cfg)
        .unwrap_or_else(|e| unreachable!("the program starts: {e}"));
    assert_eq!(s.seal_status(), Status::NotApplicable);
    assert!(s.holder_pid().is_none());
    let mut events = Vec::new();
    assert!(pump_until(&mut s, &mut events, |s, _| text(s).contains("hi")));
}

#[test]
fn killing_the_shell_ends_the_holder() {
    let Some(shell) = shell_or_skip() else { return };
    let cfg = config(1, &home("sealed-shell-kill"));
    let mut s = Session::start_with_seal(layer(), 100, 30, &shell, &[] as &[&str], &cfg)
        .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    let holder_pid = s
        .holder_pid()
        .unwrap_or_else(|| unreachable!("a holder runs"));
    let mut events = Vec::new();
    assert!(pump_until(&mut s, &mut events, |_, e| !sealed(e).is_empty()));
    assert!(s.send(b"exit\n").is_ok());
    let end = Instant::now() + Duration::from_secs(20);
    let mut gone = false;
    while Instant::now() < end {
        s.pump_wait(Duration::from_millis(100));
        // A zombie still has a /proc entry; its state says Z.
        let state = std::fs::read_to_string(format!("/proc/{holder_pid}/stat")).ok();
        if state.is_none_or(|t| {
            t.rsplit(')')
                .next()
                .is_some_and(|r| r.trim_start().starts_with('Z'))
        }) {
            gone = true;
            break;
        }
    }
    assert!(gone, "the holder outlived its shell");
}

#[test]
fn a_link_to_a_listed_shell_resolves_to_the_path_that_runs_and_other_paths_do_not() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sealed-shell-resolve");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    let real = dir.join("zsh5");
    assert!(std::fs::write(&real, b"").is_ok());
    let link = dir.join("zsh");
    assert!(std::os::unix::fs::symlink(&real, &link).is_ok());
    let canonical = std::fs::canonicalize(&real).unwrap_or_default();
    let canonical = canonical.display().to_string();
    let mut cfg = config(1, &dir);
    cfg.listed = vec![canonical.clone()];
    assert_eq!(
        lumen_term::seal::resolve(&cfg, &link.display().to_string()),
        Some(canonical.clone())
    );
    assert_eq!(lumen_term::seal::resolve(&cfg, &canonical), Some(canonical));
    // Not listed, not absolute, or not there: no sealed layer.
    cfg.listed = vec!["/usr/bin/zsh".to_owned()];
    assert_eq!(
        lumen_term::seal::resolve(&cfg, &link.display().to_string()),
        None
    );
    assert_eq!(lumen_term::seal::resolve(&cfg, "zsh"), None);
    assert_eq!(lumen_term::seal::resolve(&cfg, "/nonexistent/zsh"), None);
}
