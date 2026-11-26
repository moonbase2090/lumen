//! The input box stays on screen the whole time: at a prompt, while a command runs, while a
//! program reads a password, on the alternate screen and after the program ended. Every frame
//! drawn between those moments is read pixel by pixel for the box's top and bottom edges.

#![cfg(unix)]

use lumen_term::app::{EDITOR_ROWS, window_cells, window_for_box};
use lumen_term::input_line::Mode;
use lumen_term::{App, Frame, Key, Mods, Passthrough, Renderer};
use std::time::{Duration, Instant};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

fn start(program: &str, args: &[&str]) -> App {
    App::start(Box::new(Passthrough), program, args, 1, WIDTH, HEIGHT)
        .unwrap_or_else(|e| unreachable!("{program} starts: {e}"))
}

fn renderer() -> Renderer {
    Renderer::new(1, lumen_term::design::theme())
        .unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

/// Wait until `done` holds, drawing a frame after every take of output and
/// checking the box is in it.
fn until_drawn(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        assert_box(&app.view(), "a frame while waiting");
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

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.key(Key::Char(c), Mods::default())
            .unwrap_or_else(|e| unreachable!("key: {e}"));
    }
}

fn enter(app: &mut App) {
    app.key(Key::Enter, Mods::default())
        .unwrap_or_else(|e| unreachable!("enter: {e}"));
}

/// The box's top and bottom edges are drawn across most of its width, in the
/// edge colour (the prompt colour while it takes the keys, else the border).
fn assert_box(frame: &Frame, when: &str) {
    let theme = renderer().theme().clone();
    let (cw, ch) = renderer().cell();
    let (columns, rows) = window_cells(WIDTH, HEIGHT, &renderer());
    let win = window_for_box(columns, rows, EDITOR_ROWS);
    let top = win.editor_row * ch;
    let bottom = (win.editor_row + win.editor_rows) * ch - 1;
    for y in [top, bottom] {
        let edge = (0..WIDTH)
            .filter(|&x| {
                let p = frame.pixel(x, y);
                p == Some(theme.prompt) || p == Some(theme.border)
            })
            .count();
        assert!(
            edge > usize::try_from(WIDTH / 2 - 4 * cw).unwrap_or(usize::MAX),
            "{when}: the box's edge at row {y} is missing ({edge} edge pixels)"
        );
    }
}

#[test]
fn the_box_is_there_at_a_prompt_while_a_command_runs_and_after_it() {
    let mut app = start("/bin/zsh", &["-f", "-i"]);
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Editing));
    assert_box(&app.view(), "at the prompt");

    type_str(
        &mut app,
        "sh -c 'printf \"job-%s\\n\" started; exec sleep 30'",
    );
    enter(&mut app);
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Raw));
    assert_box(&app.view(), "while the command runs");

    app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Editing));
    assert_box(&app.view(), "back at the prompt");
}

#[test]
fn the_box_is_there_while_a_password_is_read_and_on_the_alternate_screen() {
    let mut hidden = start("/bin/sh", &["-c", "stty -echo; printf ready; exec cat"]);
    assert!(until_drawn(&mut hidden, |a| a.input_mode() == Mode::Hidden));
    assert_box(&hidden.view(), "while a password is read");

    let mut full = start("/bin/sh", &["-c", "printf '\\033[?1049hscreen'; exec cat"]);
    assert!(until_drawn(&mut full, |a| a.input_mode() == Mode::Raw));
    assert_box(&full.view(), "on the alternate screen");
}

#[test]
fn the_box_is_there_after_the_program_ended() {
    let mut app = start("/bin/sh", &["-c", "printf done"]);
    assert!(until_drawn(&mut app, App::has_exited));
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Ended));
    assert_box(&app.view(), "after the program ended");
}
