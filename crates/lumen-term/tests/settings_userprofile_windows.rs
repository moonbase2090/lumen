//! On Windows `HOME` is not usually set, so `settings::load` finds the file
//! under the user profile folder. This test sets that up for real: it starts
//! this test program again as a child with `HOME` removed and `USERPROFILE`
//! pointing at a folder that holds a settings file, and the child calls
//! `settings::load`. (The parent's environment is never changed.)

#![cfg(windows)]

use lumen_term::settings::{self, Settings};
use std::process::Command;

const CHILD: &str = "LUMEN_TEST_PROFILE_CHILD";

/// Runs only as the child: reads the settings the way the terminal does.
#[test]
fn load_in_the_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let loaded = settings::load();
    assert!(
        matches!(loaded, Settings::Loaded { .. }),
        "the file under the profile folder was read: {loaded:?}"
    );
    assert_eq!(loaded.look().shell.as_deref(), Some("pwsh"));
    println!("child-checked: settings read from the profile folder");
}

#[test]
fn load_reads_the_file_under_the_profile_folder_when_home_is_unset() {
    let profile = std::env::temp_dir().join(format!("lumen-profile-{}", std::process::id()));
    let dir = profile.join(".config").join("lumen");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("profile folder: {e}"));
    std::fs::write(dir.join("settings.toml"), "shell = \"pwsh\"\n")
        .unwrap_or_else(|e| unreachable!("settings file: {e}"));
    let exe = std::env::current_exe().unwrap_or_else(|e| unreachable!("test program: {e}"));
    let out = Command::new(exe)
        .args(["--exact", "load_in_the_child", "--nocapture"])
        .env_remove("HOME")
        .env("USERPROFILE", &profile)
        .env(CHILD, "1")
        .output()
        .unwrap_or_else(|e| unreachable!("child starts: {e}"));
    std::fs::remove_dir_all(&profile).ok();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the child failed: {said}");
    assert!(
        said.contains("child-checked"),
        "the child ran its check (it did not just skip it): {said}"
    );
}
