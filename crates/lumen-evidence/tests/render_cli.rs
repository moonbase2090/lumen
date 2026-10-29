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

#[test]
fn a_list_of_streams_is_refused_with_exit_2_not_read_as_zero() {
    let l = write(
        "rl-list.json",
        &GOOD.replace("\"streams\":2", "\"streams\":[\"a\",\"b\"]"),
    );
    assert_eq!(run(Some(&l), &tmp("rl-list")), Some(2));
}

#[test]
fn a_flood_writes_a_pipeline_log_that_the_judge_reads_as_a_proxy() {
    let out = tmp("rl-flood");
    let _ = std::fs::remove_dir_all(&out);
    let flood = Command::new(env!("CARGO_BIN_EXE_lumen-evidence"))
        .args([
            "--flood",
            "--seconds",
            "1",
            "--commit",
            "t",
            "--arm",
            "b",
            "--out",
        ])
        .arg(&out)
        .status()
        .ok()
        .and_then(|s| s.code());
    assert_eq!(flood, Some(0));
    let log = out.join("render.json");
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(text.contains("\"basis\":\"pipeline\""), "{text:.200}");
    assert!(!text.contains("lying_decoder"));
    let entries = text.matches(',').count();
    assert!(
        entries >= 20,
        "a second of returns kept at 10 ms is dozens of entries: {entries}"
    );
    let judged = tmp("rl-flood-judged");
    let shown = Command::new(env!("CARGO_BIN_EXE_lumen-evidence"))
        .args(["--render-log", "--commit", "t", "--out"])
        .arg(&judged)
        .arg("--log")
        .arg(&log)
        .output();
    let Ok(shown) = shown else {
        unreachable!("the driver runs")
    };
    assert_eq!(shown.status.code(), Some(3));
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("longest_return_gap_ms="), "{text}");
    assert!(text.contains("the UI stall is not measured"), "{text}");
}

#[test]
fn a_pipeline_log_with_a_gap_over_the_limit_exits_1() {
    let log = write(
        "rl-pipe-over.json",
        r#"{"revision":"rl-1","basis":"pipeline","build":"t","duration_ms":3000,"frames_ms":[0,10,2900]}"#,
    );
    assert_eq!(run(Some(&log), &tmp("rl-pipe-over")), Some(1));
    let within = write(
        "rl-pipe-ok.json",
        r#"{"revision":"rl-1","basis":"pipeline","build":"t","duration_ms":3000,"frames_ms":[0,1000,2000]}"#,
    );
    assert_eq!(run(Some(&within), &tmp("rl-pipe-ok")), Some(3));
}
