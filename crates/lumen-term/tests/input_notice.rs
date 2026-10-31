//! Typed input that cannot be sent is never silent: when the program is not
//! reading what the user types, the window shows the terminal's own notice, in
//! the notification area and not in the trust margin, and says so to the window
//! so that it draws again. A real program on a real PTY, through the same
//! function the window's event handler calls.

#![cfg(all(unix, feature = "window"))]

use lumen_term::app::{INPUT_NOTICE_MS, cells_for, window_for};
use lumen_term::host::{Input, send_input};
use lumen_term::notify::{Place, Settings};
use lumen_term::{
    App, Block, Frame, Key, Mods, Passthrough, Rect, Renderer, layout, notification_area,
};
use std::time::{Duration, Instant};

const W: u32 = 800;
const H: u32 = 600;

fn start(script: &str) -> App {
    App::start(Box::new(Passthrough), "/bin/sh", &["-c", script], 1, W, H)
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

fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        app.pump();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A program that puts its terminal in raw mode, says it is ready, and then
/// never reads: what a program busy elsewhere looks like to the user typing.
const NEVER_READS: &str = "stty raw -echo; echo READY; sleep 60";

/// Fill the program's input until the app refuses, and return how long the
/// slowest send took.
fn type_until_refused(app: &mut App) -> Duration {
    let paste = "x".repeat(4096);
    let mut slowest = Duration::ZERO;
    for _ in 0..4000 {
        let start = Instant::now();
        let redraw = send_input(app, Input::Text(&paste));
        slowest = slowest.max(start.elapsed());
        if redraw {
            return slowest;
        }
    }
    unreachable!("4000 pastes of 4 KiB to a program that never reads were all accepted");
}

fn geometry() -> (lumen_term::Layout, Renderer) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"));
    let (columns, rows) = cells_for(W, H, r.cell());
    let win = window_for(columns, rows);
    let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
    (lay, r)
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

#[test]
fn input_the_program_is_not_reading_is_refused_with_a_visible_notice() {
    let mut app = start(NEVER_READS);
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")));
    let before = app.view();
    assert!(!app.input_refused());
    assert_eq!(app.toasts(), []);

    let slowest = type_until_refused(&mut app);
    assert!(
        slowest < Duration::from_millis(500),
        "a send took {slowest:?}: the event thread must not wait on the program"
    );

    // The user has something to see: the app says it holds a notice, and it is
    // the terminal's fixed text with a count.
    assert!(app.input_refused());
    let toasts = app.toasts();
    assert_eq!(toasts.len(), 1);
    let text = &toasts[0].text;
    assert!(
        text.starts_with("Input not sent: the program is not reading its input (")
            && text.ends_with(" dropped)"),
        "{text:?}"
    );
    assert!(!toasts[0].unsigned, "it is the terminal's, not unverified");

    // And it is on the screen, in the notification area only.
    let after = app.view();
    let (lay, r) = geometry();
    let note = notification_area(&lay);
    assert!(changed(&r, &before, &after, &note) > 0, "nothing was drawn");
    for m in &lay.margin {
        assert_eq!(
            changed(&r, &before, &after, m),
            0,
            "the trust margin changed"
        );
    }
    assert_eq!(
        changed(&r, &before, &after, &lay.editor),
        0,
        "the editor changed"
    );
}

#[test]
fn each_refusal_is_counted_and_the_text_is_the_terminals_alone() {
    let mut app = start(NEVER_READS);
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")));
    type_until_refused(&mut app);
    let count = |a: &App| -> u64 {
        let t = a.toasts()[0].text.clone();
        let digits: String = t
            .rsplit_once('(')
            .map(|(_, n)| n.chars().take_while(char::is_ascii_digit).collect())
            .unwrap_or_default();
        digits.parse().unwrap_or(0)
    };
    let first = count(&app);
    assert!(first >= 1);
    for _ in 0..5 {
        assert!(send_input(&mut app, Input::Text("more")));
    }
    assert_eq!(count(&app), first + 5, "every refused write is counted");
    // A key is refused the same way, and the window is told to draw.
    assert!(send_input(
        &mut app,
        Input::Key(Key::Char('a'), Mods::default())
    ));
    assert_eq!(count(&app), first + 6);
}

#[test]
fn a_click_dismisses_the_notice_and_the_window_is_as_before() {
    let mut app = start(NEVER_READS);
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")));
    let before = app.view();
    type_until_refused(&mut app);
    let (lay, r) = geometry();
    let note = notification_area(&lay);
    let (cw, ch) = r.cell();
    let hit = (note.y * ch..(note.y + note.h) * ch)
        .flat_map(|y| (note.x * cw..(note.x + note.w) * cw).map(move |x| (x, y)))
        .find(|(x, y)| app.click_would_hit(*x, *y) == Some(0));
    let Some((x, y)) = hit else {
        unreachable!("the notice is drawn somewhere in the notification area")
    };
    assert!(app.click(x, y), "a press on the notice dismisses it");
    assert!(!app.input_refused());
    assert_eq!(app.toasts(), []);
    assert_eq!(app.view(), before, "the window is exactly as it was");
}

#[test]
fn the_notice_goes_away_by_itself_and_the_window_is_told() {
    let mut app = start(NEVER_READS);
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")));
    type_until_refused(&mut app);
    assert!(!app.tick(), "it has not been up long enough to go");
    let up = Instant::now();
    let mut told = false;
    while up.elapsed() < Duration::from_millis(INPUT_NOTICE_MS + 3000) {
        if app.tick() {
            told = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(told, "tick never reported that the notice was gone");
    assert!(up.elapsed() >= Duration::from_millis(INPUT_NOTICE_MS - 500));
    assert!(!app.input_refused());
}

#[test]
fn a_program_that_floods_notifications_cannot_push_the_notice_out() {
    let script = "stty raw -echo; echo READY; i=0; \
                  while [ $i -lt 400 ]; do printf '\\033]9;hi\\007'; sleep 0.1; i=$((i+1)); done";
    let mut app = start(script);
    // A layer shown local, so that the program's own toasts are drawn too.
    app.configure_notifications(
        Settings {
            allow_unsigned: true,
            ..Settings::default()
        },
        Place::Local,
        Box::new(lumen_term::app::NoSink),
    );
    assert!(pump_until(&mut app, |a| !a.notifier().shown().is_empty()));
    type_until_refused(&mut app);
    let toasts = app.toasts();
    assert!(toasts.len() >= 2, "the program's toast is there too");
    assert_eq!(toasts[0].layer, 0, "the terminal's notice is first");
    assert!(toasts[0].text.starts_with("Input not sent:"));
    // Clicking the notice dismisses only the notice.
    let before = app.notifier().shown().len();
    let (lay, r) = geometry();
    let note = notification_area(&lay);
    let (cw, ch) = r.cell();
    let hit = (note.y * ch..(note.y + note.h) * ch)
        .flat_map(|y| (note.x * cw..(note.x + note.w) * cw).map(move |x| (x, y)))
        .find(|(x, y)| app.click_would_hit(*x, *y) == Some(0));
    let Some((x, y)) = hit else {
        unreachable!("the notice is drawn")
    };
    assert!(app.click(x, y));
    assert!(!app.input_refused());
    assert_eq!(
        app.notifier().shown().len(),
        before,
        "the program's toast stays"
    );
}

#[test]
fn input_that_is_sent_asks_for_no_redraw_and_leaves_no_notice() {
    let mut app = start("stty raw -echo; echo READY; cat >/dev/null");
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")));
    assert!(!send_input(
        &mut app,
        Input::Key(Key::Char('a'), Mods::default())
    ));
    assert!(!send_input(&mut app, Input::Text("hello")));
    assert!(!app.input_refused());
    assert_eq!(app.toasts(), []);
}

#[test]
fn input_after_the_program_has_gone_is_not_silent_either() {
    let mut app = start("true");
    let end = Instant::now() + Duration::from_secs(20);
    while !app.has_exited() && Instant::now() < end {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.has_exited(), "the program should have ended");
    let end = Instant::now() + Duration::from_secs(10);
    let mut redraw = false;
    while Instant::now() < end && !redraw {
        redraw = send_input(&mut app, Input::Text("hello"));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        redraw,
        "typing to a program that is gone was never reported"
    );
    let text = &app.toasts()[0].text;
    assert!(
        text.starts_with("Input not sent: the program is no longer reading"),
        "{text:?}"
    );
}
