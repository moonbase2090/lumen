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
        check(app, "a frame while waiting");
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

/// The box is drawn: its top and bottom edges run across most of its width, in
/// the prompt colour while it takes the keys and in the border colour while
/// they go to a program. Inside its first row, the prompt colour (the `>` mark
/// and the caret) is there only while it takes the keys, and while the keys go
/// to a program the muted text is there instead.
fn assert_box(frame: &Frame, editing: bool, when: &str) {
    let theme = renderer().theme().clone();
    let (cw, ch) = renderer().cell();
    let (columns, rows) = window_cells(WIDTH, HEIGHT, &renderer());
    let win = window_for_box(columns, rows, EDITOR_ROWS);
    let top = win.editor_row * ch;
    let bottom = (win.editor_row + win.editor_rows) * ch - 1;
    let (edge_colour, other) = if editing {
        (theme.prompt, theme.border)
    } else {
        (theme.border, theme.prompt)
    };
    for y in [top, bottom] {
        let count = |colour| {
            (0..WIDTH)
                .filter(|&x| frame.pixel(x, y) == Some(colour))
                .count()
        };
        assert!(
            count(edge_colour) > usize::try_from(WIDTH / 2 - 4 * cw).unwrap_or(usize::MAX),
            "{when}: the box's edge at row {y} is not drawn in the expected colour"
        );
        assert_eq!(
            count(other),
            0,
            "{when}: the edge at row {y} is in the wrong colour"
        );
    }
    // Text is antialiased, so a pixel counts as a colour when that colour is the
    // nearest of the box's field, the prompt and the muted text.
    let near = |p: [u8; 3], c: [u8; 3]| -> i32 {
        p.iter()
            .zip(c)
            .map(|(&a, b)| (i32::from(a) - i32::from(b)).pow(2))
            .sum()
    };
    let inside = |colour: [u8; 3]| {
        let others = [theme.editor, theme.prompt, theme.muted];
        (top + 2..top + ch - 1)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter_map(|(x, y)| frame.pixel(x, y))
            .filter(|&p| others.iter().all(|&o| near(p, colour) <= near(p, o)))
            .count()
    };
    if editing {
        assert!(
            inside(theme.prompt) > 0,
            "{when}: no prompt mark in the box"
        );
    } else {
        assert_eq!(
            inside(theme.prompt),
            0,
            "{when}: something in the box is in the prompt colour"
        );
        assert!(
            inside(theme.muted) > 0,
            "{when}: no muted status in the box"
        );
    }
}

/// Draw a frame and check the box against where the keys go now.
fn check(app: &mut App, when: &str) {
    let editing = app.input_mode() == Mode::Editing;
    assert_box(&app.view(), editing, when);
}

#[test]
fn the_box_is_there_at_a_prompt_while_a_command_runs_and_after_it() {
    let mut app = start("/bin/zsh", &["-f", "-i"]);
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Editing));
    check(&mut app, "at the prompt");

    type_str(
        &mut app,
        "sh -c 'printf \"job-%s\\n\" started; exec sleep 30'",
    );
    enter(&mut app);
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Raw));
    check(&mut app, "while the command runs");

    app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Editing));
    check(&mut app, "back at the prompt");
}

#[test]
fn the_box_is_there_while_a_password_is_read_and_on_the_alternate_screen() {
    let mut hidden = start("/bin/sh", &["-c", "stty -echo; printf ready; exec cat"]);
    assert!(until_drawn(&mut hidden, |a| a.input_mode() == Mode::Hidden));
    check(&mut hidden, "while a password is read");

    let mut full = start("/bin/sh", &["-c", "printf '\\033[?1049hscreen'; exec cat"]);
    assert!(until_drawn(&mut full, |a| a.input_mode() == Mode::Raw));
    check(&mut full, "on the alternate screen");
}

#[test]
fn the_box_is_there_after_the_program_ended() {
    let mut app = start("/bin/sh", &["-c", "printf done"]);
    assert!(until_drawn(&mut app, App::has_exited));
    assert!(until_drawn(&mut app, |a| a.input_mode() == Mode::Ended));
    check(&mut app, "after the program ended");
}
