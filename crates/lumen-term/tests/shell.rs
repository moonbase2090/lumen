//! Which program the window runs when none is named: the user's setting and
//! `SHELL` first, then PowerShell and `cmd.exe` on Windows and `/bin/sh`
//! elsewhere. The choice is made by one function, so these call it with the
//! facts a machine would give it and pass on every platform.

use lumen_term::settings::{self, Settings};
use lumen_term::shell::choose;
use std::path::Path;

fn good(text: &str) -> settings::LookSettings {
    settings::parse(text, Path::new("/tmp"), None).unwrap_or_else(|e| unreachable!("{text}: {e:?}"))
}

fn path_has<'a>(names: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
    move |n| names.contains(&n)
}

#[test]
fn windows_prefers_pwsh_then_powershell_then_comspec_then_cmd() {
    let both = path_has(&["pwsh.exe", "powershell.exe"]);
    assert_eq!(
        choose(None, None, true, Some(r"C:\Windows\system32\cmd.exe"), both),
        "pwsh.exe"
    );
    let old = path_has(&["powershell.exe"]);
    assert_eq!(
        choose(None, None, true, Some(r"C:\Windows\system32\cmd.exe"), old),
        "powershell.exe"
    );
    let none = path_has(&[]);
    assert_eq!(
        choose(None, None, true, Some(r"C:\Windows\system32\cmd.exe"), none),
        r"C:\Windows\system32\cmd.exe"
    );
    assert_eq!(choose(None, None, true, None, path_has(&[])), "cmd.exe");
    assert_eq!(choose(None, None, true, Some(""), path_has(&[])), "cmd.exe");
}

#[test]
fn the_users_setting_wins_everywhere_and_then_shell() {
    let everything = path_has(&["pwsh.exe", "powershell.exe"]);
    for windows in [true, false] {
        assert_eq!(
            choose(
                Some("nu"),
                Some("/bin/zsh"),
                windows,
                Some("cmd.exe"),
                &everything
            ),
            "nu"
        );
        assert_eq!(
            choose(
                None,
                Some("/usr/bin/bash"),
                windows,
                Some("cmd.exe"),
                &everything
            ),
            "/usr/bin/bash"
        );
    }
}

#[test]
fn an_empty_or_blank_setting_or_shell_counts_as_not_set() {
    assert_eq!(
        choose(Some(""), Some("  "), false, None, path_has(&[])),
        "/bin/sh"
    );
    assert_eq!(
        choose(Some(" "), Some(""), true, None, path_has(&["pwsh.exe"])),
        "pwsh.exe"
    );
}

#[test]
fn other_systems_keep_shell_then_bin_sh_and_never_ask_for_powershell() {
    assert_eq!(
        choose(None, Some("/bin/zsh"), false, None, |_| true),
        "/bin/zsh"
    );
    let never = |n: &str| -> bool {
        unreachable!("a program was looked up on a system that is not Windows: {n}")
    };
    assert_eq!(choose(None, None, false, Some("cmd.exe"), never), "/bin/sh");
}

#[test]
fn the_shell_setting_is_read_from_the_settings_file() {
    assert_eq!(good("shell = \"pwsh\"\n").shell.as_deref(), Some("pwsh"));
    assert_eq!(good("").shell, None);
    let with_path = good(r#"shell = "C:\Program Files\PowerShell\7\pwsh.exe""#);
    assert_eq!(
        with_path.shell.as_deref(),
        Some(r"C:\Program Files\PowerShell\7\pwsh.exe")
    );
}

#[test]
fn a_bad_shell_setting_ignores_the_file_with_a_notice() {
    for (text, why) in [
        ("shell = \"\"", "`shell` must be a program name"),
        ("shell = \"  \"", "`shell` must be a program name"),
        ("shell = pwsh", "`shell` needs a quoted value"),
        ("shell = \"a\"\nshell = \"b\"", "`shell` is given twice"),
    ] {
        let Err(err) = settings::parse(text, Path::new("/tmp"), None) else {
            unreachable!("{text} was accepted");
        };
        assert!(err.1.contains(why), "{text}: {}", err.1);
    }
}

#[test]
fn a_loaded_file_hands_the_setting_on() {
    let tmp = std::env::temp_dir().join(format!("lumen-shell-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap_or_else(|e| unreachable!("scratch directory: {e}"));
    let file = tmp.join("settings.toml");
    std::fs::write(&file, "shell = \"pwsh\"\n").unwrap_or_else(|e| unreachable!("write: {e}"));
    let loaded = settings::load_from(&file, None);
    assert!(matches!(loaded, Settings::Loaded { .. }));
    assert_eq!(loaded.look().resolve().shell.as_deref(), Some("pwsh"));
    std::fs::remove_dir_all(&tmp).ok();
    assert_eq!(Settings::Defaults.look().resolve().shell, None);
}

#[test]
fn the_home_is_home_or_on_windows_the_profile_folder() {
    use lumen_term::settings::home_dir;
    use std::ffi::OsString;
    use std::path::PathBuf;
    let os = |s: &str| Some(OsString::from(s));
    // HOME wins where it is set, on every system.
    for windows in [true, false] {
        assert_eq!(
            home_dir(os("/home/a"), os(r"C:\Users\a"), windows),
            Some(PathBuf::from("/home/a"))
        );
    }
    // The profile folder stands in only on Windows, and only when HOME is not set or empty.
    assert_eq!(
        home_dir(None, os(r"C:\Users\a"), true),
        Some(PathBuf::from(r"C:\Users\a"))
    );
    assert_eq!(
        home_dir(os(""), os(r"C:\Users\a"), true),
        Some(PathBuf::from(r"C:\Users\a"))
    );
    assert_eq!(home_dir(None, os(r"C:\Users\a"), false), None);
    assert_eq!(home_dir(None, os(""), true), None);
    assert_eq!(home_dir(None, None, true), None);
}
