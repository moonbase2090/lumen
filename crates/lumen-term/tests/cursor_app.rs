//! The cursor in the window's logic: blink timing, focus and the end of the program.

#![cfg(unix)]

use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        640,
        400,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
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

fn blinking(app: &App) -> bool {
    app.session().document().blocks().iter().any(|b| match b {
        Block::Grid(g) => g.cursor().blink && g.text_rows().iter().any(|r| r == "hi"),
        Block::Rich(_) => false,
    })
}

#[test]
fn a_blinking_cursor_turns_off_and_on_and_asks_for_a_new_frame_each_time() {
    let mut app = start("printf 'hi\\033[1 q'; sleep 8");
    assert!(
        pump_until(&mut app, blinking),
        "the program set a blinking cursor"
    );
    let on = app.view();
    let mut seen_off = None;
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end && seen_off.is_none() {
        if app.tick() {
            seen_off = Some(app.view());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let off = seen_off.unwrap_or_else(|| unreachable!("a new frame was asked for within 3 s"));
    assert_ne!(on, off, "the cursor went out");
    let mut back = None;
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end && back.is_none() {
        if app.tick() {
            back = Some(app.view());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(back, Some(on), "and came back as it was");
}

#[test]
fn an_unfocused_window_stops_the_blink_and_dims_the_cursor() {
    let mut app = start("printf 'hi\\033[1 q'; sleep 8");
    assert!(pump_until(&mut app, blinking));
    let focused = app.view();
    app.set_focused(false);
    let dim = app.view();
    assert_ne!(focused, dim, "the cursor is dimmed at once");
    let end = Instant::now() + Duration::from_millis(1300);
    while Instant::now() < end {
        assert!(!app.tick(), "an unfocused cursor does not ask for frames");
        assert_eq!(app.view(), dim, "and does not change");
        std::thread::sleep(Duration::from_millis(50));
    }
    app.set_focused(true);
    assert_ne!(app.view(), dim, "focus brings it back");
}

#[test]
fn the_cursor_goes_when_the_program_ends() {
    let mut app = start("printf 'hi'");
    assert!(pump_until(&mut app, |a| a
        .session()
        .document()
        .blocks()
        .iter()
        .any(
            |b| matches!(b, Block::Grid(g) if g.text_rows().iter().any(|r| r == "hi"))
        )));
    let running = app.view();
    assert!(pump_until(&mut app, |_| true));
    let end = Instant::now() + Duration::from_secs(10);
    while !app.has_exited() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.has_exited());
    assert_ne!(
        app.view(),
        running,
        "the cursor of a finished program is not drawn"
    );
}

#[test]
fn composed_or_pasted_text_brings_a_blinking_cursor_back_on() {
    let mut app = start("stty -echo; printf 'hi\\033[1 q'; sleep 8");
    assert!(pump_until(&mut app, blinking));
    let on = app.view();
    let end = Instant::now() + Duration::from_secs(3);
    let mut off = false;
    while Instant::now() < end && !off {
        off = app.tick() && app.view() != on;
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(off, "the cursor went out");
    app.text("x")
        .unwrap_or_else(|e| unreachable!("sh reads: {e}"));
    assert_eq!(app.view(), on, "text restarts the cycle at its on half");
}

#[test]
fn less_motion_brings_a_cursor_caught_off_back_on_and_says_so_once() {
    let mut app = start("printf 'hi\\033[1 q'; sleep 8");
    assert!(pump_until(&mut app, blinking));
    let on = app.view();
    assert!(
        !app.set_reduce_motion(false),
        "nothing changes when the setting stays as it was"
    );
    // Whatever half the cycle is in, asking for less motion leaves the cursor drawn as it is when on.
    let _ = app.set_reduce_motion(true);
    assert_eq!(app.view(), on);
    assert!(!app.set_reduce_motion(true), "asking again changes nothing");
}
