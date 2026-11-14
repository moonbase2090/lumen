//! The window's own actions do what their names say, on a real program on a
//! real pseudo-terminal. The clipboard is a stand-in with fixed text; the
//! system clipboard itself is exercised in the platform proofs.

#![cfg(unix)]

use lumen_term::actions::{Action, Held, Platform, action_for};
use lumen_term::app::Outcome;
use lumen_term::clipboard::Clipboard;
use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

struct Fixed(Option<&'static str>);

impl Clipboard for Fixed {
    fn text(&self) -> Option<String> {
        self.0.map(str::to_owned)
    }
}

fn text(app: &App) -> String {
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

fn start(script: &str, scale: u32) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        scale,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

#[test]
fn paste_sends_the_clipboard_text_to_the_program() {
    let mut app = start("read line; echo got:$line", 1);
    let outcome = app.perform(Action::Paste, &Fixed(Some("hello world")));
    assert_eq!(outcome, Outcome::Redraw);
    assert_eq!(app.perform(Action::Paste, &Fixed(None)), Outcome::Nothing);
    app.text("\n").ok();
    assert!(pump_until(&mut app, |a| text(a).contains("got:hello world")));
}

#[test]
fn a_multi_line_paste_reaches_the_program_as_one_line_and_runs_nothing() {
    let mut app = start(
        "read line; echo got:[$line]; read more; echo more:[$more]",
        1,
    );
    app.perform(Action::Paste, &Fixed(Some("one\ntwo\r\nthree")));
    app.text("\n").ok();
    assert!(pump_until(&mut app, |a| text(a).contains("got:[one two three]")));
}

#[test]
fn a_paste_carrying_a_frame_introducer_is_refused_and_nothing_is_sent() {
    let mut app = start("read line; echo got:[$line]", 1);
    let outcome = app.perform(Action::Paste, &Fixed(Some("before\x1b_after")));
    assert_eq!(outcome, Outcome::Refused);
    app.text("ok\n").ok();
    assert!(pump_until(&mut app, |a| text(a).contains("got:[ok]")));
}

/// The last `stty size` line on screen as (rows, columns).
fn size_line(app: &App) -> Option<(u32, u32)> {
    text(app).lines().rev().find_map(|l| {
        let mut n = l.split_whitespace().map(str::parse::<u32>);
        match (n.next(), n.next(), n.next()) {
            (Some(Ok(r)), Some(Ok(c)), None) => Some((r, c)),
            _ => None,
        }
    })
}

#[test]
fn zoom_changes_the_size_the_running_program_is_told() {
    let mut app = start("while read x; do stty size; done", 2);
    let none = Fixed(None);
    app.text("\n").ok();
    assert!(pump_until(&mut app, |a| size_line(a).is_some()));
    let at_two = size_line(&app).unwrap_or_default();
    assert_eq!(app.perform(Action::ZoomIn, &none), Outcome::Redraw);
    assert_eq!(app.scale(), 3);
    app.text("\n").ok();
    assert!(pump_until(&mut app, |a| size_line(a).is_some_and(|s| s != at_two)));
    let at_three = size_line(&app).unwrap_or_default();
    assert!(
        at_three.0 < at_two.0 && at_three.1 < at_two.1,
        "{at_two:?} {at_three:?}"
    );
    assert_eq!(app.perform(Action::ZoomReset, &none), Outcome::Redraw);
    assert_eq!(app.scale(), 2);
    app.text("\n").ok();
    assert!(pump_until(&mut app, |a| size_line(a) == Some(at_two)));
    assert_eq!(app.perform(Action::ZoomOut, &none), Outcome::Redraw);
    assert_eq!(
        app.perform(Action::ZoomOut, &none),
        Outcome::Nothing,
        "scale 1 is the smallest"
    );
    assert_eq!(app.view().height, 600, "the window keeps its pixel size");
}

#[test]
fn quit_and_close_end_the_application() {
    let mut app = start("sleep 5", 1);
    assert_eq!(app.perform(Action::Quit, &Fixed(None)), Outcome::Exit);
    assert_eq!(app.perform(Action::Close, &Fixed(None)), Outcome::Exit);
}

#[test]
fn the_shortcut_for_each_action_is_the_one_the_platform_uses() {
    let command = Held {
        command: true,
        ..Held::default()
    };
    let linux = Held {
        control: true,
        shift: true,
        ..Held::default()
    };
    for (key, action) in [
        ('v', Action::Paste),
        ('=', Action::ZoomIn),
        ('-', Action::ZoomOut),
        ('0', Action::ZoomReset),
        ('w', Action::Close),
        ('q', Action::Quit),
    ] {
        assert_eq!(action_for(Platform::Mac, key, command), Some(action));
    }
    assert_eq!(action_for(Platform::Other, 'v', linux), Some(Action::Paste));
    assert_eq!(
        action_for(Platform::Other, '+', linux),
        Some(Action::ZoomIn)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn every_macos_menu_item_stands_for_one_action_and_back() {
    use lumen_term::menu::{action_of, id_of};
    for action in [
        Action::Paste,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
        Action::Close,
        Action::Quit,
    ] {
        assert_eq!(action_of(id_of(action)), Some(action));
    }
    assert_eq!(action_of("not-an-item"), None);
}
