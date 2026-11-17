//! Moving the window between displays of different scale: the render scale
//! follows the display, a zoom stays the same steps away, and the frame is the
//! one a window started on that display would draw. A real program on a real
//! PTY; no window is opened.

#![cfg(unix)]

use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

const SHOW: &str = "printf 'Hello, sharp text\\n'; trap 'echo size:$(stty size)' WINCH; while :; do sleep 0.05; done";

fn start(scale: u32, width: u32, height: u32) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", SHOW],
        scale,
        width,
        height,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
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

fn shown(app: &mut App) {
    assert!(
        pump_until(app, |a| text(a).contains("Hello, sharp text")),
        "{}",
        text(app)
    );
}

/// The program's window size as it last reported it: `rows columns`.
fn reported(app: &App) -> Option<(usize, usize)> {
    let t = text(app);
    let last = t
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("size:"))?;
    let mut it = last.split_whitespace().map(|n| n.parse::<usize>().ok());
    Some((it.next()??, it.next()??))
}

#[test]
fn a_move_to_a_sharper_display_draws_what_a_window_started_there_draws() {
    let mut moved = start(1, 800, 500);
    shown(&mut moved);
    let one = moved.cell_height();
    assert_eq!(moved.set_display_scale(2.0).ok(), Some(true));
    assert_eq!(moved.scale(), 2);
    assert!(moved.cell_height() >= 2 * one - 1, "cells are drawn at 2x");

    let mut native = start(2, 800, 500);
    shown(&mut native);
    assert_eq!(moved.cell_height(), native.cell_height());
    // Neither blurry nor double scaled: the same pixels, at the window's size.
    let (a, b) = (moved.view(), native.view());
    assert_eq!((a.width, a.height), (800, 500));
    assert!(a == b, "the moved window's frame differs from a native one");
}

#[test]
fn the_program_is_told_the_new_window_size_after_each_move() {
    let mut app = start(1, 800, 500);
    shown(&mut app);
    assert!(app.set_display_scale(2.0).is_ok());
    assert!(
        pump_until(&mut app, |a| reported(a).is_some()),
        "{}",
        text(&app)
    );
    let (rows2, cols2) = reported(&app).unwrap_or_else(|| unreachable!("reported"));
    assert!(app.set_display_scale(1.0).is_ok());
    assert!(
        pump_until(&mut app, |a| reported(a).is_some_and(|(_, c)| c > cols2)),
        "{}",
        text(&app)
    );
    let (rows1, cols1) = reported(&app).unwrap_or_else(|| unreachable!("reported"));
    assert!(
        cols1 > cols2 && rows1 > rows2,
        "{rows1}x{cols1} vs {rows2}x{cols2}"
    );
}

#[test]
fn a_zoom_stays_the_same_steps_from_the_display_across_a_move() {
    let mut app = start(1, 800, 500);
    assert!(app.set_scale(2).is_ok(), "zoomed in one step");
    assert_eq!(app.set_display_scale(2.0).ok(), Some(true));
    assert_eq!(app.scale(), 3, "one step above the new display's scale");
    assert_eq!(app.set_display_scale(1.0).ok(), Some(true));
    assert_eq!(app.scale(), 2);
    // The same factor again changes nothing.
    assert_eq!(app.set_display_scale(1.0).ok(), Some(false));
}

#[test]
fn a_fractional_factor_rounds_and_a_wild_one_is_kept_in_range() {
    let mut app = start(1, 800, 500);
    assert_eq!(app.set_display_scale(1.25).ok(), Some(false));
    assert_eq!(app.scale(), 1);
    assert_eq!(app.set_display_scale(1.5).ok(), Some(true));
    assert_eq!(app.scale(), 2);
    assert!(app.set_display_scale(100.0).is_ok());
    assert_eq!(app.scale(), 4);
    assert!(app.set_display_scale(f64::NAN).is_ok());
    assert_eq!(app.scale(), 1);
    assert!(app.set_display_scale(0.0).is_ok());
    assert_eq!(app.scale(), 1);
}

#[test]
fn a_scale_fixed_by_the_user_does_not_follow_the_display() {
    let mut app = start(3, 800, 500);
    app.pin_scale();
    assert_eq!(app.set_display_scale(1.0).ok(), Some(false));
    assert_eq!(app.scale(), 3);
}
