//! On Windows `HOME` is not usually set, so `settings::load` finds the file
//! under the user profile folder. This test sets that up for real: it runs in
//! a process of its own, because it changes the environment.

#![cfg(windows)]

use lumen_term::settings::{self, Settings};

#[test]
fn load_reads_the_file_under_the_profile_folder_when_home_is_unset() {
    let profile = std::env::temp_dir().join(format!("lumen-profile-{}", std::process::id()));
    let dir = profile.join(".config").join("lumen");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("profile folder: {e}"));
    std::fs::write(dir.join("settings.toml"), "shell = \"pwsh\"\n")
        .unwrap_or_else(|e| unreachable!("settings file: {e}"));
    // SAFETY: this test is the only one in its process, so nothing else reads or
    // writes the environment while it runs.
    unsafe {
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", &profile);
    }
    let loaded = settings::load();
    std::fs::remove_dir_all(&profile).ok();
    assert!(
        matches!(loaded, Settings::Loaded { .. }),
        "the file under the profile folder was read: {loaded:?}"
    );
    assert_eq!(loaded.look().shell.as_deref(), Some("pwsh"));
}
