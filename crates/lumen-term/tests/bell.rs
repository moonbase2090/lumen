//! The bell: a real program behind a real PTY rings it, and the window asks
//! the system for attention only while it is not focused, once, and takes the
//! request back when the user returns. A BEL that ends a string is not a bell.

#![cfg(unix)]

use lumen_term::attention::Step;
use lumen_term::{App, Passthrough};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (640, 444);

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

/// Pump until the program has ended and its output has been read to the end,
/// then collect every non-idle step.
fn steps_until_exit(app: &mut App) -> Vec<Step> {
    let mut seen = Vec::new();
    let mut note = |app: &mut App| match app.attention_step() {
        Step::Idle => {}
        step => seen.push(step),
    };
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end && !app.has_exited() {
        app.pump();
        note(app);
        std::thread::sleep(Duration::from_millis(20));
    }
    // The program has ended; what it wrote may still be on its way.
    let drained = Instant::now() + Duration::from_millis(500);
    while Instant::now() < drained {
        app.pump();
        note(app);
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

#[test]
fn a_bell_in_the_background_asks_for_attention_once() {
    let mut app = start("printf 'a\\007b\\007c\\007'");
    app.set_focused(false);
    assert_eq!(steps_until_exit(&mut app), vec![Step::Request]);
}

#[test]
fn a_bell_in_the_focused_window_asks_for_nothing() {
    let mut app = start("printf 'ring\\007'");
    app.set_focused(true);
    assert_eq!(steps_until_exit(&mut app), Vec::<Step>::new());
}

#[test]
fn coming_back_takes_the_request_away() {
    let mut app = start("printf 'ring\\007'");
    app.set_focused(false);
    assert_eq!(steps_until_exit(&mut app), vec![Step::Request]);
    app.set_focused(true);
    assert_eq!(app.attention_step(), Step::Cancel);
    assert_eq!(app.attention_step(), Step::Idle);
}

#[test]
fn a_bell_that_ends_a_title_is_not_a_bell() {
    let mut app = start("printf '\\033]2;build\\007text\\n'");
    app.set_focused(false);
    assert_eq!(steps_until_exit(&mut app), Vec::<Step>::new());
}

#[test]
fn no_bell_means_no_request() {
    let mut app = start("printf 'quiet\\n'");
    app.set_focused(false);
    assert_eq!(steps_until_exit(&mut app), Vec::<Step>::new());
}
