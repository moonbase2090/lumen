//! A policy, settings or theme file the terminal cannot use is told to the
//! user in the window, not only on standard error: a notice in the
//! notification area, ahead of every toast, that stays while the defaults apply.
//! Real files on disk, read by the same functions the binary calls, and a real
//! program behind a real PTY.

#![cfg(unix)]
// Short names for frames, rectangles and cells keep the comparisons readable.
#![allow(clippy::many_single_char_names)]

use lumen_term::app::{MAX_CONFIG_NOTICES, cells_for, window_for};
use lumen_term::notify::{Place, Settings as Notifications};
use lumen_term::policy::{Policy, load_from};
use lumen_term::settings::{Settings, Watcher, notices};
use lumen_term::{App, Block, Frame, Passthrough, Rect, Renderer, layout, notification_area};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (640, 444);

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lumen-config-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    assert!(std::fs::create_dir_all(&d).is_ok());
    assert!(std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).is_ok());
    d
}

fn write(path: &Path, text: &str, mode: u32) {
    // A file left unreadable by an earlier case cannot be written over.
    let _ = std::fs::remove_file(path);
    assert!(std::fs::write(path, text).is_ok());
    assert!(std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok());
}

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

fn grid_text(app: &App) -> String {
    app.session()
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wait for `done`: after each check the test blocks until the program writes
/// again. The bound only ends a hung test; there is no polling interval.
fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if done(app) {
            return true;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        app.pump_wait(left);
    }
}

fn area() -> (Renderer, Rect, lumen_term::Layout) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("face loads: {e}"));
    let (c, rows) = cells_for(SIZE.0, SIZE.1, r.cell());
    let w = window_for(c, rows);
    let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
    let a = notification_area(&l);
    (r, a, l)
}

fn changed(r: &Renderer, a: &Frame, b: &Frame, rect: &Rect) -> usize {
    let (cw, ch) = r.cell();
    let mut n = 0;
    for y in rect.y * ch..(rect.y + rect.h) * ch {
        for x in rect.x * cw..(rect.x + rect.w) * cw {
            if a.pixel(x, y) != b.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

/// What the window does at start: the notices of the files, given to the app.
fn show(app: &mut App, policy: &Policy, settings: &Settings) -> bool {
    let look = settings.look().resolve();
    app.set_config_notices(notices(policy.notice().as_deref(), settings, &look, false))
}

fn texts(app: &App) -> Vec<String> {
    app.toasts().iter().map(|t| t.text.clone()).collect()
}

#[test]
fn a_policy_file_that_is_unreadable_world_writable_or_malformed_gives_a_notice_and_the_defaults() {
    let d = dir("policy");
    let f = d.join("policy.toml");
    let cases: [(&str, &str, u32, &str); 3] = [
        (
            "unreadable",
            "os_notifications = true\n",
            0o000,
            "could not be read",
        ),
        (
            "world-writable",
            "os_notifications = true\n",
            0o666,
            "allows writes by group or others",
        ),
        ("malformed", "os_notifications = yes\n", 0o600, "line 1"),
    ];
    for (name, text, mode, why) in cases {
        write(&f, text, mode);
        let policy = load_from(&f);
        assert!(
            matches!(policy, Policy::Ignored { .. }),
            "{name}: the file is ignored"
        );
        assert_eq!(
            policy.settings(),
            Notifications::default(),
            "{name}: the defaults are in force"
        );
        let mut app = start("sleep 30");
        assert_eq!(
            app.toasts(),
            [],
            "{name}: nothing before the window shows it"
        );
        show(&mut app, &policy, &Settings::Defaults);
        let shown = texts(&app);
        assert_eq!(shown.len(), 1, "{name}: {shown:?}");
        assert!(
            shown[0].starts_with("policy file ") && shown[0].contains("ignored"),
            "{name}: {shown:?}"
        );
        assert!(shown[0].contains(why), "{name}: {shown:?}");
        assert!(
            shown[0].contains("the defaults apply"),
            "{name}: says the defaults apply: {shown:?}"
        );
        // The mode is put back so the directory can be removed.
        write(&f, text, 0o600);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_valid_policy_file_gives_no_notice() {
    let d = dir("valid");
    let f = d.join("policy.toml");
    write(&f, "os_notifications = true\n", 0o600);
    let policy = load_from(&f);
    assert!(matches!(policy, Policy::Loaded { .. }));
    assert!(
        policy.settings().os_notifications,
        "the file's setting is in force"
    );
    let mut app = start("sleep 30");
    show(&mut app, &policy, &Settings::Defaults);
    assert_eq!(app.toasts(), []);
    assert_eq!(app.config_notices(), [] as [String; 0]);
    // No file at all is not an ignored file either.
    show(&mut app, &Policy::Defaults, &Settings::Defaults);
    assert_eq!(app.toasts(), []);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_refused_settings_or_theme_file_shows_its_text_in_the_window() {
    let d = dir("settings");
    let f = d.join("settings.toml");
    // A settings file the terminal refuses as a whole.
    write(&f, "theme = nonsense\n", 0o600);
    let watcher = Watcher::from_path(f.clone(), Some(d.clone()));
    let mut app = start("sleep 30");
    show(&mut app, &Policy::Defaults, watcher.current());
    let shown = texts(&app);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(
        shown[0].starts_with("settings file ") && shown[0].contains("the defaults apply"),
        "{shown:?}"
    );
    assert_eq!(
        Some(shown[0].clone()),
        watcher.current().notice(),
        "the window shows the same text the terminal printed"
    );

    // A theme file the settings name that cannot be used: the settings load,
    // the theme is left out, and the window says so.
    let missing = d.join("no-such-theme.toml");
    write(&f, &format!("theme = \"{}\"\n", missing.display()), 0o600);
    let watcher = Watcher::from_path(f.clone(), Some(d.clone()));
    show(&mut app, &Policy::Defaults, watcher.current());
    let shown = texts(&app);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(
        shown[0].starts_with("theme file ") && shown[0].contains("not used"),
        "{shown:?}"
    );

    // Fixing the file takes the notice away.
    write(&f, "theme = \"dark\"\n", 0o600);
    let watcher = Watcher::from_path(f, Some(d.clone()));
    show(&mut app, &Policy::Defaults, watcher.current());
    assert_eq!(app.toasts(), []);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_notice_is_drawn_in_the_notification_area_and_nowhere_else() {
    let (r, a, l) = area();
    let mut app = start("sleep 30");
    let before = app.view();
    app.set_config_notices(vec![
        "policy file here ignored (it could not be read); the defaults apply".to_owned(),
    ]);
    let after = app.view();
    assert!(
        changed(&r, &before, &after, &a) > 0,
        "the notice is drawn in the notification area"
    );
    for m in &l.margin {
        assert_eq!(
            changed(&r, &before, &after, m),
            0,
            "the trust margin is untouched"
        );
    }
    assert_eq!(
        changed(&r, &before, &after, &l.editor),
        0,
        "so is the editor"
    );
}

#[test]
fn a_flood_of_toasts_and_clearing_output_do_not_push_the_notice_out() {
    let (r, a, _) = area();
    let (cw, ch) = r.cell();
    // The program asks for two hundred notifications, clears and resets the
    // screen, then says it is done.
    let mut app = start(
        "i=0; while [ $i -lt 200 ]; do printf '\\033]9;flood %s\\007' $i; i=$((i+1)); done; \
         printf '\\033[2J\\033[3J\\033[H\\033c'; echo FLOODED; sleep 30",
    );
    app.configure_notifications(
        Notifications {
            allow_unsigned: true,
            ..Notifications::default()
        },
        Place::Local,
        Box::new(lumen_term::notify::Recorder::default()),
    );
    let notice = "policy file here ignored (it is not valid); the defaults apply";
    app.set_config_notices(vec![notice.to_owned()]);
    assert!(
        pump_until(&mut app, |x| grid_text(x).contains("FLOODED")),
        "the program finished its flood"
    );
    assert!(
        app.notifier().dropped() > 0,
        "the flood was real: the terminal dropped some of it"
    );
    let shown = texts(&app);
    assert_eq!(shown[0], notice, "the notice is still first: {shown:?}");
    // It is drawn at the top of the area, where a click would hit it first.
    assert_eq!(
        app.click_would_hit(a.x * cw + 2 * cw, a.y * ch + ch),
        Some(0)
    );
    // And the user cannot hide it by clicking either: the file is still ignored.
    assert!(!app.click(a.x * cw + 2 * cw, a.y * ch + ch));
    assert_eq!(app.config_notices(), [notice.to_owned()]);
}

#[test]
fn only_a_bounded_number_of_notices_are_kept_and_a_new_list_replaces_the_old() {
    let mut app = start("sleep 30");
    let many: Vec<String> = (0..MAX_CONFIG_NOTICES + 3)
        .map(|i| format!("notice {i}"))
        .collect();
    app.set_config_notices(many.clone());
    assert_eq!(app.config_notices(), &many[..MAX_CONFIG_NOTICES]);
    app.set_config_notices(vec!["one".to_owned()]);
    assert_eq!(app.config_notices(), ["one".to_owned()]);
    app.set_config_notices(Vec::new());
    assert_eq!(app.toasts(), []);
}

#[test]
fn a_reload_that_changes_only_the_notice_says_the_window_must_draw_again() {
    let d = dir("reload");
    let f = d.join("settings.toml");
    // The edits below leave the theme and the fonts at their defaults: only the
    // notice differs, so only the notice can ask for a new frame.
    write(&f, "theme = nonsense\n", 0o600);
    let mut watcher = Watcher::from_path(f.clone(), Some(d.clone()));
    let mut app = start("sleep 30");
    assert!(
        show(&mut app, &Policy::Defaults, watcher.current()),
        "the first notice"
    );
    assert_eq!(app.config_notices().len(), 1);
    let ignored_look = watcher.current().look();

    // Fixed to a file that says the defaults: the warning must go.
    write(&f, "# the defaults\n", 0o600);
    let fixed = watcher.take_change().cloned();
    let Some(fixed) = fixed else {
        unreachable!("the watcher saw the edit")
    };
    assert_eq!(fixed.look(), ignored_look, "theme and fonts did not change");
    assert!(
        show(&mut app, &Policy::Defaults, &fixed),
        "clearing it needs a redraw"
    );
    assert_eq!(app.toasts(), []);
    assert!(
        !show(&mut app, &Policy::Defaults, &fixed),
        "nothing new, nothing to draw"
    );

    // And back to malformed: the new warning must be drawn.
    write(&f, "theme = nonsense\n", 0o600);
    let broken = watcher.take_change().cloned();
    let Some(broken) = broken else {
        unreachable!("the watcher saw the edit")
    };
    assert_eq!(
        broken.look(),
        ignored_look,
        "theme and fonts did not change"
    );
    assert!(
        show(&mut app, &Policy::Defaults, &broken),
        "a new warning needs a redraw"
    );
    assert_eq!(app.config_notices().len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_long_notice_is_drawn_to_its_last_line_while_a_programs_toast_is_cut_at_three() {
    let (r, a, _) = area();
    let (cw, ch) = r.cell();
    let (c, rows) = cells_for(SIZE.0, SIZE.1, r.cell());
    let win = window_for(c, rows);
    // A real ignored policy file whose path is long, as a home directory and
    // a nested configuration directory make it: the notice wraps to more than
    // three lines of the area.
    let d = dir("long");
    let nested = d.join("a-configuration-directory-with-a-long-name/and-another-one");
    assert!(std::fs::create_dir_all(&nested).is_ok());
    let f = nested.join("policy.toml");
    write(&f, "os_notifications = true\n", 0o666);
    let Some(text) = load_from(&f).notice() else {
        unreachable!("a world-writable policy file is ignored")
    };
    let _ = std::fs::remove_dir_all(&d);
    let inner = usize::try_from(a.w - 2).unwrap_or(1);
    let lines = lumen_term::wrap_text(&text, inner).len();
    assert!(lines > 4, "the notice wraps to {lines} lines");

    let toast = |whole| lumen_term::notify::Toast {
        layer: 0,
        text: text.clone(),
        unsigned: false,
        shown_ms: 0,
        whole,
    };
    let x = a.x * cw + 2 * cw;
    // The middle of text line `n` (0-based) of the first toast's box.
    let line = |n: u32| a.y * ch + 1 + n * ch + ch / 2;
    let last = u32::try_from(lines).unwrap_or(0) - 1;

    let notice = [toast(true)];
    for n in 0..=last {
        assert_eq!(
            r.toast_at(&win, &notice, x, line(n)),
            Some(0),
            "line {n} of {lines} is part of the notice's box"
        );
    }
    assert_eq!(
        r.toast_at(&win, &notice, x, line(last + 1)),
        None,
        "the box ends after its last line"
    );

    let program = [toast(false)];
    assert_eq!(r.toast_at(&win, &program, x, line(2)), Some(0));
    assert_eq!(
        r.toast_at(&win, &program, x, line(3)),
        None,
        "a program's toast is still cut at three lines"
    );

    // What is drawn is what is hit: the notice paints its last line, where
    // the program's toast of the same text leaves the window's background.
    let doc = lumen_term::Document::new(
        usize::try_from(win.cols).unwrap_or(1),
        usize::try_from(win.rows).unwrap_or(1),
    );
    let empty = r.render_window_with(&doc, &win, &[]);
    let whole = r.render_window_with(&doc, &win, &notice);
    let cut = r.render_window_with(&doc, &win, &program);
    let last_row = Rect {
        x: a.x,
        y: a.y + last,
        w: a.w,
        h: 1,
    };
    assert!(changed(&r, &empty, &whole, &last_row) > 0);
    assert_eq!(changed(&r, &empty, &cut, &last_row), 0);
}

#[test]
fn a_pane_that_does_not_draw_the_notices_keeps_them_for_when_it_does() {
    let (r, a, _) = area();
    let (cw, ch) = r.cell();
    let mut app = start("sleep 30");
    let notice = "policy file here ignored (it is not valid); the defaults apply".to_owned();
    app.set_config_notices(vec![notice.clone()]);
    let with = app.view();
    assert!(
        app.set_draws_config_notices(false),
        "hiding a shown notice asks for a new frame"
    );
    assert_eq!(app.toasts(), [], "another pane draws it");
    assert_eq!(app.click_would_hit(a.x * cw + 2 * cw, a.y * ch + ch), None);
    let without = app.view();
    assert!(
        changed(&r, &with, &without, &a) > 0,
        "and it is not painted"
    );
    assert_eq!(
        app.config_notices(),
        std::slice::from_ref(&notice),
        "it is kept"
    );
    assert!(
        !app.set_config_notices(vec!["another".to_owned()]),
        "a change this pane does not draw needs no frame"
    );
    assert!(app.set_draws_config_notices(true));
    assert_eq!(texts(&app), ["another".to_owned()]);
    assert!(!app.set_draws_config_notices(true), "no change, no frame");
}

#[test]
fn a_terminal_started_from_ssh_draws_a_notice_about_images() {
    let mut app = start("sleep 30");
    let look = Settings::Defaults.look().resolve();
    assert!(app.set_config_notices(notices(None, &Settings::Defaults, &look, true)));
    let shown = app.config_notices();
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(
        shown[0].contains("SSH session") && shown[0].contains("alt text"),
        "{shown:?}"
    );
    assert!(
        !app.set_config_notices(notices(None, &Settings::Defaults, &look, true)),
        "the same notice needs no new frame"
    );
    assert!(
        app.set_config_notices(notices(None, &Settings::Defaults, &look, false)),
        "a terminal that is not on SSH has none"
    );
    assert_eq!(app.config_notices(), [] as [String; 0]);
}
