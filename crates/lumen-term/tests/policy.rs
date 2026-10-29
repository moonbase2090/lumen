//! The policy file: where it is read from, what it may say, and when it is ignored.

use lumen_term::policy::{Ignored, MAX_BYTES, Policy, load_as, load_from, parse};

#[test]
fn the_defaults_are_all_off_and_a_file_may_turn_settings_on() {
    let d = parse("").unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(
        (d.allow_unsigned, d.os_notifications, d.os_text),
        (false, false, false)
    );
    let s = parse("# my policy\nallow_unsigned = true   # a comment\n\nos_notifications = true\nos_text = false\n")
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(
        (s.allow_unsigned, s.os_notifications, s.os_text),
        (true, true, false)
    );
}

#[test]
fn anything_the_parser_does_not_understand_rejects_the_whole_file() {
    let cases = [
        ("allow_unsigned = yes", 1),
        ("allow_unsigned", 1),
        ("os_text = true\nunknown = true", 2),
        ("os_text = true\nos_text = false", 2),
        ("[notifications]\nos_text = true", 1),
        ("allow_unsigned = \"true\"", 1),
        ("allow_unsigned = 1", 1),
        ("Allow_Unsigned = true", 1),
        ("os_notifications = true;", 1),
    ];
    for (text, line) in cases {
        match parse(text) {
            Err((n, _)) => assert_eq!(n, line, "{text:?}"),
            Ok(s) => unreachable!("{text:?} was accepted as {s:?}"),
        }
    }
}

#[cfg(unix)]
mod files {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumen-policy-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(std::fs::create_dir_all(&d).is_ok());
        assert!(std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).is_ok());
        d
    }

    fn write(path: &Path, text: &str, mode: u32) {
        assert!(std::fs::write(path, text).is_ok());
        assert!(std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok());
    }

    fn why(p: &Policy) -> Option<&Ignored> {
        match p {
            Policy::Ignored { why, .. } => Some(why),
            _ => None,
        }
    }

    #[test]
    fn no_file_means_the_defaults_and_no_notice() {
        let d = dir("none");
        let p = load_from(&d.join("policy.toml"));
        assert_eq!(p, Policy::Defaults);
        assert_eq!(p.notice(), None);
        let s = p.settings();
        assert_eq!(
            (s.allow_unsigned, s.os_notifications, s.os_text),
            (false, false, false)
        );
    }

    #[test]
    fn a_user_owned_private_file_is_read() {
        let d = dir("ok");
        let f = d.join("policy.toml");
        for mode in [0o600, 0o644, 0o400] {
            write(&f, "allow_unsigned = true\nos_notifications = true\n", mode);
            let p = load_from(&f);
            assert!(matches!(p, Policy::Loaded { .. }), "mode {mode:o}: {p:?}");
            let s = p.settings();
            assert_eq!(
                (s.allow_unsigned, s.os_notifications, s.os_text),
                (true, true, false)
            );
            assert_eq!(p.notice(), None);
        }
    }

    #[test]
    fn a_file_owned_by_someone_else_is_ignored() {
        use std::os::unix::fs::MetadataExt;
        let d = dir("owner");
        let f = d.join("policy.toml");
        write(&f, "os_notifications = true\n", 0o600);
        let mine = std::fs::metadata(&f).map_or(0, |m| m.uid());
        assert!(matches!(load_as(&f, mine), Policy::Loaded { .. }));
        let p = load_as(&f, mine.wrapping_add(1));
        assert_eq!(why(&p), Some(&Ignored::NotOwned));
        assert!(!p.settings().os_notifications);
    }

    #[test]
    fn a_file_writable_by_group_or_others_is_ignored_with_a_notice() {
        let d = dir("writable");
        let f = d.join("policy.toml");
        for mode in [0o620, 0o602, 0o666, 0o664, 0o606] {
            write(&f, "os_notifications = true\n", mode);
            let p = load_from(&f);
            assert_eq!(why(&p), Some(&Ignored::Writable), "mode {mode:o}");
            assert!(!p.settings().os_notifications, "the defaults apply");
            let notice = p.notice().unwrap_or_default();
            assert!(
                notice.contains("ignored") && notice.contains("defaults apply"),
                "{notice}"
            );
        }
    }

    #[test]
    fn a_directory_that_others_can_write_to_lets_them_swap_the_file_so_it_is_ignored() {
        let d = dir("dirw");
        let f = d.join("policy.toml");
        write(&f, "os_notifications = true\n", 0o600);
        assert!(std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).is_ok());
        let p = load_from(&f);
        assert_eq!(why(&p), Some(&Ignored::DirectoryWritable));
        assert!(!p.settings().os_notifications);
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }

    #[test]
    fn a_symbolic_link_is_ignored_even_to_a_good_file() {
        let d = dir("link");
        let real = d.join("real.toml");
        write(&real, "os_notifications = true\n", 0o600);
        let link = d.join("policy.toml");
        assert!(std::os::unix::fs::symlink(&real, &link).is_ok());
        let p = load_from(&link);
        assert_eq!(why(&p), Some(&Ignored::NotRegular));
        assert!(!p.settings().os_notifications);
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_ignored() {
        let d = dir("isdir");
        let f = d.join("policy.toml");
        assert!(std::fs::create_dir(&f).is_ok());
        assert_eq!(why(&load_from(&f)), Some(&Ignored::NotRegular));
    }

    #[test]
    fn a_large_file_and_a_bad_line_are_ignored_and_name_the_line() {
        let d = dir("bad");
        let f = d.join("policy.toml");
        write(
            &f,
            &"# padding\n".repeat(1 + usize::try_from(MAX_BYTES).unwrap_or(0) / 9),
            0o600,
        );
        assert_eq!(why(&load_from(&f)), Some(&Ignored::TooLarge));
        write(&f, "os_text = true\nos_notifcations = true\n", 0o600);
        let p = load_from(&f);
        assert!(
            matches!(why(&p), Some(Ignored::Invalid { line: 2, .. })),
            "{p:?}"
        );
        assert!(p.notice().unwrap_or_default().contains("line 2"));
        assert!(!p.settings().os_text, "a half-read policy is not used");
        std::fs::write(&f, [0xff, 0xfe, 0x00]).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).is_ok());
        assert!(matches!(why(&load_from(&f)), Some(Ignored::Invalid { .. })));
    }

    // ---- the real binary ----

    fn binary(home: &Path, args: &[&str]) -> (String, String) {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
            .env("HOME", home)
            .args(args)
            .output()
            .unwrap_or_else(|e| unreachable!("the binary runs: {e}"));
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn home_with(name: &str, text: Option<&str>, mode: u32) -> PathBuf {
        let home = dir(name);
        let conf = home.join(".config/lumen");
        assert!(std::fs::create_dir_all(&conf).is_ok());
        for d in [home.join(".config"), conf.clone()] {
            assert!(std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).is_ok());
        }
        if let Some(t) = text {
            write(&conf.join("policy.toml"), t, mode);
        }
        home
    }

    #[test]
    fn the_binary_reports_the_file_it_read_the_defaults_or_why_it_ignored_the_file() {
        let (out, err) = binary(&home_with("b-none", None, 0o600), &["--show-policy"]);
        assert!(out.contains("no file") && out.contains("all off"), "{out}");
        assert_eq!(err, "");

        let (out, _) = binary(
            &home_with(
                "b-ok",
                Some("allow_unsigned = true\nos_text = true\n"),
                0o600,
            ),
            &["--show-policy"],
        );
        assert!(
            out.contains("allow_unsigned=true os_notifications=false os_text=true"),
            "{out}"
        );

        let (out, _) = binary(
            &home_with("b-open", Some("os_notifications = true\n"), 0o666),
            &["--show-policy"],
        );
        assert!(
            out.contains("ignored") && out.contains("group or others"),
            "{out}"
        );
        assert!(!out.contains("os_notifications=true"), "{out}");
    }

    #[test]
    fn an_ignored_file_is_noticed_on_a_normal_run_and_a_good_one_is_silent() {
        let args = ["--size", "200x100", "--wait", "3000", "--snapshot"];
        let png = std::env::temp_dir().join(format!("lumen-policy-{}.png", std::process::id()));
        let png = png.to_string_lossy().into_owned();
        let run = |home: &Path| {
            let mut a: Vec<&str> = args.to_vec();
            a.extend([png.as_str(), "--", "/bin/sh", "-c", "true"]);
            binary(home, &a).1
        };
        let err = run(&home_with("n-open", Some("os_text = true\n"), 0o666));
        assert!(
            err.contains("ignored") && err.contains("the defaults apply"),
            "{err}"
        );
        assert_eq!(
            run(&home_with("n-good", Some("os_text = true\n"), 0o600)),
            ""
        );
        assert_eq!(run(&home_with("n-absent", None, 0o600)), "");
    }

    #[test]
    fn the_environment_and_the_working_directory_do_not_supply_policy() {
        let home = home_with("e-none", None, 0o600);
        // A file in the working directory and variables that name settings.
        let cwd = dir("e-cwd");
        write(&cwd.join("policy.toml"), "os_notifications = true\n", 0o600);
        write(
            &cwd.join(".config.toml"),
            "os_notifications = true\n",
            0o600,
        );
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
            .current_dir(&cwd)
            .env("HOME", &home)
            .env("os_notifications", "true")
            .env("LUMEN_OS_NOTIFICATIONS", "1")
            .env("XDG_CONFIG_HOME", &cwd)
            .arg("--show-policy")
            .output()
            .unwrap_or_else(|e| unreachable!("the binary runs: {e}"));
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            text.contains("no file") && text.contains("all off"),
            "{text}"
        );
    }
}
