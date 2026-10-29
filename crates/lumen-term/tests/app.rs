//! The window's logic, without a window.

#![cfg(unix)]

use lumen_term::app::cells_for;
use lumen_term::{App, Block, Frame, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

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

fn start(script: &str, width: u32, height: u32) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        width,
        height,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

#[test]
fn pixels_make_cells_within_the_limits() {
    assert_eq!(cells_for(800, 600, (8, 19)), (100, 31));
    assert_eq!(
        cells_for(7, 18, (8, 19)),
        (1, 1),
        "a tiny window is one cell"
    );
    assert_eq!(cells_for(0, 0, (8, 19)), (1, 1));
    assert_eq!(cells_for(100_000, 100_000, (8, 19)), (1000, 500), "capped");
    assert_eq!(
        cells_for(80, 38, (0, 0)),
        (80, 38),
        "a zero cell does not divide by zero"
    );
}

#[test]
fn the_window_size_sets_the_ptys_size() {
    let mut app = start("stty size", 800, 600);
    assert!(pump_until(&mut app, |a| text(a).contains("31 100")));
}

#[test]
fn resizing_the_window_resizes_the_program() {
    let mut app = start("echo first; read x; stty size", 800, 600);
    assert!(pump_until(&mut app, |a| text(a).contains("first")));
    assert!(app.resize(400, 380).is_ok());
    let view = app.view();
    assert_eq!(
        (view.width, view.height),
        (400, 380),
        "the view follows the window"
    );
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("20 50")));
}

#[test]
fn keys_reach_the_program() {
    let mut app = start("read x; echo got:$x", 640, 380);
    for c in "hey".chars() {
        assert!(app.key(Key::Char(c), Mods::default()).is_ok());
    }
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("got:hey")));
}

#[test]
fn control_c_reaches_the_program_as_a_signal() {
    let mut app = start(
        "trap 'echo interrupted; exit 0' INT; echo ready; while :; do sleep 1; done",
        640,
        380,
    );
    assert!(pump_until(&mut app, |a| text(a).contains("ready")));
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert!(app.key(Key::Char('c'), ctrl).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("interrupted")));
}

#[test]
fn pasted_text_is_sent_as_it_is() {
    let mut app = start("read x; echo got:$x", 640, 380);
    assert!(app.text("pasted").is_ok());
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("got:pasted")));
}

#[test]
fn the_view_is_exactly_the_window_size_and_shows_the_programs_text() {
    let mut app = start("echo shown", 640, 380);
    assert!(pump_until(&mut app, |a| text(a).contains("shown")));
    let view = app.view();
    assert_eq!((view.width, view.height), (640, 380));
    assert!(
        (0..38).any(|y| (0..640).any(|x| view.pixel(x, y) != Some([255, 255, 255]))),
        "ink in the first row"
    );
}

#[test]
fn the_program_exiting_is_seen() {
    let mut app = start("true", 320, 190);
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !app.has_exited() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.has_exited());
    assert!(app.has_exited(), "asking again still says so");
}

#[test]
fn a_viewport_shows_the_top_of_a_short_frame_and_the_bottom_of_a_tall_one() {
    let mut f = Frame::new(4, 6, [255, 255, 255]);
    for (y, v) in (0..6).zip([0u8, 10, 20, 30, 40, 50]) {
        f.fill(0, y, 4, 1, [v, 0, 0]);
    }
    let short = f.viewport(6, 8, [9, 9, 9]);
    assert_eq!((short.width, short.height), (6, 8));
    assert_eq!(short.pixel(0, 0), Some([0, 0, 0]), "top aligned");
    assert_eq!(short.pixel(0, 5), Some([50, 0, 0]));
    assert_eq!(short.pixel(0, 6), Some([9, 9, 9]), "below the frame");
    assert_eq!(short.pixel(5, 0), Some([9, 9, 9]), "right of the frame");
    let tall = f.viewport(4, 3, [9, 9, 9]);
    assert_eq!((tall.width, tall.height), (4, 3));
    assert_eq!(tall.pixel(0, 0), Some([30, 0, 0]), "the last three rows");
    assert_eq!(tall.pixel(0, 2), Some([50, 0, 0]));
    let narrow = f.viewport(2, 6, [9, 9, 9]);
    assert_eq!(narrow.pixel(1, 3), Some([30, 0, 0]), "cropped on the right");
}
