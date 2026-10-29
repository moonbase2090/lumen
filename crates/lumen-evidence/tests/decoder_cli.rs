//! The exit status of `--decoder`: 0 only when the gate was measured and holds.

use std::path::{Path, PathBuf};
use std::process::Command;

fn out_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn run(args: &[&str]) -> Option<i32> {
    Command::new(env!("CARGO_BIN_EXE_lumen-evidence"))
        .args(args)
        .status()
        .ok()
        .and_then(|s| s.code())
}

fn ledger(name: &str, text: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    assert!(std::fs::write(&path, text).is_ok());
    path
}

const GOOD: &str = r#"{"revision":"fz-1","decoder_build":"d1","image_path_build":"p1",
 "formats":[{"name":"png","cpu_hours":1200,"crashes":[],
  "coverage":[{"at_cpu_hour":100,"edges":900},{"at_cpu_hour":1200,"edges":900}]}],
 "sandbox_escape":{"revision":"se-1","passed":3,"total":3}}"#;

#[test]
fn no_ledger_exits_3() {
    let out = out_dir("dec-none");
    assert_eq!(
        run(&[
            "--decoder",
            "--commit",
            "t",
            "--out",
            &out.to_string_lossy()
        ]),
        Some(3)
    );
}

#[test]
fn a_ledger_with_no_format_exits_3() {
    let out = out_dir("dec-empty");
    let l = ledger("dec-empty.json", r#"{"formats":[]}"#);
    let args = [
        "--decoder",
        "--ledger",
        &l.to_string_lossy(),
        "--commit",
        "t",
        "--out",
        &out.to_string_lossy(),
    ];
    assert_eq!(run(&args), Some(3));
}

#[test]
fn a_passing_ledger_exits_0_and_a_failing_one_exits_1() {
    let out = out_dir("dec-pass");
    let l = ledger("dec-good.json", GOOD);
    let args = [
        "--decoder",
        "--ledger",
        &l.to_string_lossy(),
        "--commit",
        "t",
        "--out",
        &out.to_string_lossy(),
    ];
    assert_eq!(run(&args), Some(0));
    let out = out_dir("dec-fail");
    let l = ledger("dec-bad.json", &GOOD.replace("1200", "999"));
    let args = [
        "--decoder",
        "--ledger",
        &l.to_string_lossy(),
        "--commit",
        "t",
        "--out",
        &out.to_string_lossy(),
    ];
    assert_eq!(run(&args), Some(1));
}
