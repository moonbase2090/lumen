//! The exit status of `--render-log`: 0 only when every part was measured and holds.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn run(log: Option<&Path>, out: &Path) -> Option<i32> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lumen-evidence"));
    cmd.args(["--render-log", "--commit", "t", "--out"])
        .arg(out);
    if let Some(l) = log {
        cmd.arg("--log").arg(l);
    }
    cmd.status().ok().and_then(|s| s.code())
}

fn write(name: &str, text: &str) -> PathBuf {
    let path = tmp(name);
    assert!(std::fs::write(&path, text).is_ok());
    path
}

const GOOD: &str = r#"{"revision":"rl-1","build":"t1","duration_ms":4000,
 "frames_ms":[500,1500,2500,3500],
 "lying_decoder":{"decoder_build":"d1","streams":2,"peak_rss_mib":100}}"#;

#[test]
fn no_log_exits_3() {
    assert_eq!(run(None, &tmp("rl-none")), Some(3));
}

#[test]
fn a_log_within_every_limit_exits_0() {
    let l = write("rl-good.json", GOOD);
    assert_eq!(run(Some(&l), &tmp("rl-good")), Some(0));
}

#[test]
fn a_stall_over_2_seconds_exits_1() {
    let l = write(
        "rl-stall.json",
        &GOOD.replace("[500,1500,2500,3500]", "[500,3500]"),
    );
    assert_eq!(run(Some(&l), &tmp("rl-stall")), Some(1));
}

#[test]
fn a_log_without_a_lying_decoder_run_exits_3() {
    let (head, _) = GOOD
        .split_once(",\n \"lying_decoder\"")
        .unwrap_or((GOOD, ""));
    let l = write("rl-nodec.json", &format!("{head}}}"));
    assert_eq!(run(Some(&l), &tmp("rl-nodec")), Some(3));
}

#[test]
fn a_malformed_log_exits_2() {
    let l = write("rl-bad.json", "{}");
    assert_eq!(run(Some(&l), &tmp("rl-bad")), Some(2));
}
