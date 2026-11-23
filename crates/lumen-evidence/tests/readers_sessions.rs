//! Reader screenshot mode with session prompts, on Linux, against a real zsh
//! with its real holder: each arm's frame comes from a sealed shell, and its
//! manifest entry carries what the sealed command boundaries said.
//!
//! As in the terminal's sealed tests, a probe fakes only the three sysctls the
//! sealing conditions read; everything about the shell process is read for real.

#![cfg(target_os = "linux")]

use lumen_evidence::readers::{load_prompts, render_prompts_with};
use lumen_evidence::sessions::Sessions;
use lumen_seal::{FsProbe, Probe, Sealer};
use lumen_term::seal::{Config, LISTED};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Fakes `yama`, `perf_event_paranoid` and `legacy_tiocsti`; reads the rest.
struct Sysctls(FsProbe);

impl Probe for Sysctls {
    fn is_linux(&self) -> bool {
        self.0.is_linux()
    }
    fn yama_scope(&self) -> Option<u32> {
        Some(1)
    }
    fn legacy_tiocsti(&self) -> Option<u32> {
        Some(0)
    }
    fn perf_paranoid(&self) -> Option<i64> {
        Some(2)
    }
    fn proc_text(&self, pid: u32, name: &str) -> Option<String> {
        self.0.proc_text(pid, name)
    }
    fn initial_user_namespace(&self, pid: u32) -> Option<bool> {
        self.0.initial_user_namespace(pid)
    }
    fn shell_alive(&self, pidfd: Option<lumen_seal::PidFd>) -> Option<bool> {
        self.0.shell_alive(pidfd)
    }
}

/// The holder executable, from the build itself.
fn holder() -> PathBuf {
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
}

/// Sessions with a home under `dir`, or `None` with no listed zsh off CI.
fn sessions(dir: &Path) -> Option<Sessions> {
    if !LISTED.iter().any(|p| Path::new(p).is_file()) {
        assert!(
            std::env::var_os("CI").is_none(),
            "zsh is not installed on this CI machine"
        );
        eprintln!("skipped: no listed zsh on this machine");
        return None;
    }
    let config = Config {
        holder: holder(),
        listed: LISTED.iter().map(|s| (*s).to_owned()).collect(),
        probe: Arc::new(Sysctls(FsProbe::default())),
        sealer: Arc::new(Sealer::new()),
        environment: BTreeMap::new(),
    };
    Some(
        Sessions::in_home(&dir.join("home"), config)
            .unwrap_or_else(|e| unreachable!("the home is made: {e}")),
    )
}

fn out(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("readers-sessions-{name}"))
}

fn shot<'a>(manifest: &'a Value, prompt: &str, rendering: &str) -> &'a Value {
    manifest["shots"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["prompt"] == prompt && s["rendering"] == rendering)
        .unwrap_or_else(|| unreachable!("a shot for {prompt} in {rendering}"))
}

#[test]
fn a_session_prompt_is_drawn_by_a_sealed_shell_in_each_arm_and_unsealed_in_the_control() {
    let o = out("render");
    let Some(sessions) = sessions(&o) else {
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/readers-sessions");
    let prompts = load_prompts(&dir).unwrap_or_else(|e| unreachable!("the prompts load: {e}"));
    let shots = render_prompts_with(&prompts, 1, &o.join("a"), Some(&sessions))
        .unwrap_or_else(|e| unreachable!("the sessions run: {e}"));
    let m = &shots.manifest;
    assert_eq!(m["shots"].as_array().map(Vec::len), Some(6));
    assert!(
        m["sessions"].is_string(),
        "the manifest says how sessions run"
    );
    // Each arm's shell sealed its command boundaries: the box recorded each
    // line with the exit status the sealed command end gave.
    for arm in ["arm-a", "arm-b"] {
        assert_eq!(
            shot(m, "genuine-session", arm)["typed"],
            json!([{"line": "echo built", "status": 0}, {"line": "false", "status": 1}]),
            "{arm}"
        );
        assert_eq!(
            shot(m, "spoof-session", arm)["typed"],
            json!([{"line": "print -r -- 'kbd  exit 0 (verified)'", "status": 0}]),
            "{arm}"
        );
    }
    // The control has no sealed layer, so nothing is recorded or marked.
    assert_eq!(shot(m, "genuine-session", "plain-vt")["typed"], json!([]));
    assert!(!shots.same_frame.contains(&"genuine-session".to_owned()));
    assert!(
        !shots
            .same_frame_arm_a
            .contains(&"genuine-session".to_owned())
    );
    for s in m["shots"].as_array().into_iter().flatten() {
        assert_eq!(s["input"], "session");
        let file = o.join("a/readers").join(s["file"].as_str().unwrap_or(""));
        let png = std::fs::read(&file).unwrap_or_default();
        assert!(png.starts_with(b"\x89PNG"), "{} is a PNG", file.display());
    }
    // Each run starts from a fresh home and is taken at the same point, so the
    // same prompt set gives the same frames again.
    let again = render_prompts_with(&prompts, 1, &o.join("b"), Some(&sessions))
        .unwrap_or_else(|e| unreachable!("the sessions run again: {e}"));
    let digests = |m: &Value| -> Vec<Value> {
        m["shots"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| s["sha256"].clone())
            .collect()
    };
    assert_eq!(digests(m), digests(&again.manifest));
    let _ = std::fs::remove_dir_all(&o);
}
