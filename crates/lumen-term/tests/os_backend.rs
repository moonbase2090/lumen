//! The operating system's notification: what is started, and with what.

use lumen_term::notify::{
    Desk, Notifier, OsNotification, OsSink, Outcome, Place, Settings, Source, escape_markup,
};
use lumen_term::os_backend::{
    Invocation, Launch, Platform, SystemSink, TITLE, WINDOWS_TEXT_VARIABLE, invocation,
    unescape_markup,
};
use std::ffi::OsString;

#[cfg(unix)]
use lumen_term::os_backend::{MAX_IN_FLIGHT, Process};

const PLATFORMS: [Platform; 3] = [Platform::MacOs, Platform::Linux, Platform::Windows];

fn hostile() -> Vec<&'static str> {
    vec![
        "\"; do shell script \"touch /tmp/pwned\"; \"",
        "$(touch /tmp/pwned)",
        "`touch /tmp/pwned`",
        "'; end tell; tell application \"Finder\" to quit; '",
        "--urgency=critical",
        "-e do shell script \"id\"",
        "a\nb\r\nc\td",
        "</text></binding></visual></toast><script>",
        "&amp;&lt;&gt;&quot;&#39; & &unknown;",
        "\u{202e}reversed\u{200b}",
        "",
    ]
}

fn note(text: &str) -> OsNotification {
    OsNotification {
        layer: 7,
        text: escape_markup(text),
        token: 424_242_424,
    }
}

fn as_strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_text_is_one_argument_or_one_variable_and_never_part_of_a_script() {
    for text in hostile() {
        let n = note(text);
        for platform in PLATFORMS {
            let i = invocation(platform, &n);
            let args = as_strings(&i.args);
            let in_args = args.iter().filter(|a| a.contains(&n.text)).count();
            match platform {
                Platform::Windows => {
                    assert!(
                        n.text.is_empty() || in_args == 0,
                        "{platform:?} {text:?}: the text must not be in the arguments"
                    );
                    assert_eq!(
                        i.env,
                        vec![(WINDOWS_TEXT_VARIABLE.to_owned(), unescape_markup(&n.text))]
                    );
                }
                Platform::Linux | Platform::MacOs => {
                    // The text is the last argument, whole and alone.
                    let want = if platform == Platform::MacOs {
                        unescape_markup(&n.text)
                    } else {
                        n.text.clone()
                    };
                    assert_eq!(args.last(), Some(&want), "{platform:?} {text:?}");
                    assert_eq!(i.env.len(), 0);
                    // And in no other argument.
                    if !n.text.is_empty() && n.text.len() > 3 {
                        let others = &args[..args.len() - 1];
                        assert!(
                            others.iter().all(|a| !a.contains(&want)),
                            "{platform:?} {text:?}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
#[test]
#[ignore = "live proof requires a dedicated Windows test profile"]
fn hostile_windows_text_is_data_and_runs_nothing() {
    use std::process::{Command, Stdio};

    let dir = std::env::temp_dir().join(format!("lumen-os-{}", std::process::id()));
    assert!(std::fs::create_dir_all(&dir).is_ok());
    let marker = dir.join("injection-ran");
    let mut text = String::from("double \" single ' XML <tag> & command ");
    text.push('$');
    text.push('(');
    text.push_str(&["Set", "-", "Content"].concat());
    text.push_str(" -LiteralPath $env:LUMEN_INJECTION_MARKER -Value ran)");
    text.push_str(" backticks ``");
    let inv = invocation(Platform::Windows, &note(&text));
    assert_eq!(
        inv.env,
        vec![(WINDOWS_TEXT_VARIABLE.to_owned(), text.clone())]
    );
    assert!(
        as_strings(&inv.args).iter().all(|arg| !arg.contains(&text)),
        "notification text must not be part of the script"
    );

    let output = Command::new(&inv.program)
        .args(&inv.args)
        .envs(inv.env.iter().map(|(key, value)| (key, value)))
        .env("LUMEN_INJECTION_MARKER", &marker)
        .stdin(Stdio::null())
        .output();
    assert!(
        output.as_ref().is_ok_and(|output| output.status.success()),
        "Windows notification failed: {output:?}"
    );
    assert!(!marker.exists(), "notification text ran as a command");
    println!("toast text: {text}");
    println!("PowerShell exited successfully; injection marker absent");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_scripts_and_the_program_are_the_same_whatever_the_text() {
    let base: Vec<Invocation> = PLATFORMS
        .iter()
        .map(|p| invocation(*p, &note("plain")))
        .collect();
    for text in hostile() {
        for (platform, plain) in PLATFORMS.iter().zip(&base) {
            let i = invocation(*platform, &note(text));
            assert_eq!(i.program, plain.program, "{platform:?}");
            let (a, b) = (as_strings(&i.args), as_strings(&plain.args));
            let keep = if *platform == Platform::Windows {
                a.len()
            } else {
                a.len() - 1
            };
            assert_eq!(
                a[..keep],
                b[..keep],
                "{platform:?} {text:?}: only the text may differ"
            );
        }
    }
}

#[test]
fn the_programs_are_fixed_absolute_paths_started_without_a_shell() {
    for platform in PLATFORMS {
        let i = invocation(platform, &note("x"));
        let p = i.program.to_string_lossy().into_owned();
        assert!(
            p.starts_with('/') || p.starts_with("C:\\Windows\\System32\\"),
            "{p}"
        );
        assert!(!p.ends_with("sh") && !p.contains("cmd"), "{p}");
    }
    let linux = as_strings(&invocation(Platform::Linux, &note("-x")).args);
    let dashes = linux.iter().position(|a| a == "--");
    assert_eq!(
        dashes,
        Some(1),
        "options end before the title and the text: {linux:?}"
    );
    assert_eq!(linux[2], TITLE);

    let windows = as_strings(&invocation(Platform::Windows, &note("x")).args);
    assert_eq!(
        &windows[..3],
        &["-NoProfile", "-NonInteractive", "-Command"]
    );
    assert_eq!(windows.len(), 4);
    assert!(windows[3].contains("CreateTextNode($env:LUMEN_NOTIFICATION_TEXT)"));
    assert!(!windows[3].contains("<text>' + $env:"));
}

#[test]
fn the_click_token_and_the_layer_never_reach_the_operating_system() {
    let n = note("done");
    for platform in PLATFORMS {
        let i = invocation(platform, &n);
        let all = format!("{:?} {:?}", i.args, i.env);
        assert!(!all.contains("424242424"), "{platform:?}: {all}");
    }
}

#[test]
fn unescape_undoes_the_escape_exactly_for_any_text() {
    let mut state = 12_345_u64;
    let alphabet: Vec<char> = "&<>\"'; amp lt gt quot #39 x\u{4e2d}\n".chars().collect();
    for _ in 0..500 {
        let mut s = String::new();
        for _ in 0..40 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            s.push(alphabet[(state >> 33) as usize % alphabet.len()]);
        }
        assert_eq!(unescape_markup(&escape_markup(&s)), s, "{s:?}");
    }
    // Text that was never escaped is left alone where it is not an entity.
    assert_eq!(unescape_markup("a & b &x; &amp"), "a & b &x; &amp");
}

/// Records what it was asked to start, or fails.
#[derive(Default)]
struct Fake {
    started: Vec<Invocation>,
    fail: bool,
}

impl Launch for Fake {
    fn launch(&mut self, invocation: &Invocation) -> Result<(), String> {
        if self.fail {
            return Err("no such program".to_owned());
        }
        self.started.push(invocation.clone());
        Ok(())
    }
}

#[test]
fn a_sink_counts_what_it_showed_and_what_it_could_not() {
    let mut sink = SystemSink::with(Some(Platform::Linux), Fake::default());
    sink.send(&note("one"));
    sink.send(&note("two"));
    assert_eq!((sink.shown(), sink.failed()), (2, 0));
    assert_eq!(sink.launcher().started.len(), 2);

    let mut broken = SystemSink::with(
        Some(Platform::Linux),
        Fake {
            fail: true,
            ..Fake::default()
        },
    );
    broken.send(&note("lost"));
    assert_eq!(
        (broken.shown(), broken.failed()),
        (0, 1),
        "dropped and counted, not retried"
    );

    let mut none = SystemSink::with(None, Fake::default());
    none.send(&note("nowhere"));
    assert_eq!((none.shown(), none.failed()), (0, 1));
    assert_eq!(none.launcher().started.len(), 0);
}

fn focused(focused: bool) -> Desk {
    Desk {
        focused,
        muted: false,
    }
}

#[test]
fn with_the_setting_off_nothing_is_ever_started() {
    for desk in [focused(true), focused(false)] {
        let mut n = Notifier::new(Settings::default());
        n.set_desk(desk);
        n.add_layer(1, "tab 1", Place::Local);
        let mut sink = SystemSink::with(Some(Platform::Linux), Fake::default());
        for t in 0..20 {
            if let Outcome::Os(note) = n.notify(t * 2000, 1, Source::Sealed, "hello") {
                sink.send(&note);
            }
        }
        assert!(
            sink.launcher().started.is_empty(),
            "the default setting is off"
        );
    }
}

#[test]
fn with_the_setting_on_one_notification_starts_one_program_with_the_composed_text() {
    let mut n = Notifier::new(Settings {
        os_notifications: true,
        os_text: true,
        ..Settings::default()
    });
    n.set_desk(focused(false));
    n.add_layer(1, "tab 1", Place::Local);
    let mut sink = SystemSink::with(Some(Platform::Linux), Fake::default());
    let outcome = n.notify(0, 1, Source::Sealed, "build <done> & \u{202e}green");
    let Outcome::Os(note) = outcome else {
        unreachable!("the OS path applies: {outcome:?}")
    };
    sink.send(&note);
    let started = &sink.launcher().started;
    assert_eq!(started.len(), 1);
    let text = started[0]
        .args
        .last()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(text.starts_with("Program in tab 1 says:"), "{text}");
    assert!(
        text.contains("&lt;done&gt; &amp;") && !text.contains('\u{202e}'),
        "{text}"
    );
}

// ---- a real process ----

#[cfg(unix)]
mod process {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn wait_for(file: &Path) -> String {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if let Ok(s) = std::fs::read_to_string(file)
                && !s.is_empty()
            {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        String::new()
    }

    #[test]
    fn hostile_text_reaches_a_real_process_as_data_and_runs_nothing() {
        let dir = std::env::temp_dir().join(format!("lumen-os-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&dir).is_ok());
        let out = dir.join("out.txt");
        let marker = dir.join("marker");
        let text = format!(
            "$(touch {m}) `touch {m}` ; touch {m} \"'",
            m = marker.display()
        );
        let inv = Invocation {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!(
                    "printf '%s|%s' \"$1\" \"$LUMEN_TEST_TEXT\" > '{}'",
                    out.display()
                )
                .into(),
                "sh".into(),
                text.clone().into(),
            ],
            env: vec![("LUMEN_TEST_TEXT".to_owned(), text.clone())],
        };
        assert!(Process::default().launch(&inv).is_ok());
        assert_eq!(wait_for(&out), format!("{text}|{text}"));
        assert!(!marker.exists(), "the text was run as a command");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_an_error_and_the_number_in_flight_is_bounded() {
        let mut p = Process::default();
        let missing = Invocation {
            program: "/nonexistent/notify".into(),
            args: vec![],
            env: vec![],
        };
        assert!(p.launch(&missing).is_err());
        let sleeper = Invocation {
            program: "/bin/sleep".into(),
            args: vec!["2".into()],
            env: vec![],
        };
        for _ in 0..MAX_IN_FLIGHT {
            assert!(p.launch(&sleeper).is_ok());
        }
        assert!(
            p.launch(&sleeper).is_err(),
            "no more than {MAX_IN_FLIGHT} at once"
        );
        // They finish and the room comes back.
        std::thread::sleep(Duration::from_millis(2600));
        assert!(p.launch(&sleeper).is_ok());
    }
}
