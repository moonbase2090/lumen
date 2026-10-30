//! The time-boxing helper kills the whole process group, including a descendant that
//! ignores the polite signal after its parent has already gone.

#![allow(missing_docs)]

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/decoder-timebox.py")
}

fn pid_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn a_command_that_finishes_keeps_its_status() -> Result<(), Box<dyn Error>> {
    let ok = Command::new("python3")
        .arg(helper())
        .args(["5", "true"])
        .status()?;
    assert_eq!(ok.code(), Some(0));
    let bad = Command::new("python3")
        .arg(helper())
        .args(["5", "sh", "-c", "exit 3"])
        .status()?;
    assert_eq!(bad.code(), Some(3));
    Ok(())
}

#[test]
fn a_command_over_its_limit_is_killed_with_status_124() -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    let st = Command::new("python3")
        .arg(helper())
        .args(["1", "sleep", "30"])
        .status()?;
    assert_eq!(st.code(), Some(124));
    assert!(start.elapsed() < Duration::from_secs(15));
    Ok(())
}

#[test]
fn a_descendant_that_ignores_term_is_killed_after_its_parent_exits() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("timebox-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let pidfile = dir.join("child.pid");
    // The parent starts a child that ignores TERM and records its pid, then the
    // parent exits at once on TERM; only the child is left in the group.
    let script = format!(
        "sh -c 'trap \"\" TERM; echo $$ > {p}; while :; do sleep 1; done' & \
         trap 'exit 0' TERM; while :; do sleep 1; done",
        p = pidfile.display()
    );
    let st = Command::new("python3")
        .arg(helper())
        .args(["1", "sh", "-c", &script])
        .env("TIMEBOX_GRACE_SECONDS", "2")
        .status()?;
    assert_eq!(st.code(), Some(124));
    let pid = std::fs::read_to_string(&pidfile)?.trim().to_string();
    std::thread::sleep(Duration::from_millis(300));
    let alive = pid_alive(&pid);
    if alive {
        // Do not leave it running if the assertion is about to fail.
        let _ = Command::new("kill").args(["-9", &pid]).status();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!alive, "the descendant {pid} survived the time box");
    Ok(())
}
