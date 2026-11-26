//! The `input_box` setting against a real zsh on a real PTY. With the box off,
//! keys at the prompt go to the shell as typed and the program gets the rows
//! the box took; a line already in the box when it is turned off is kept and
//! sent only by its Enter; turned back on, the box takes the keys again.

#![cfg(unix)]

use lumen_term::app::EDITOR_ROWS;
use lumen_term::input_line::Mode;
use lumen_term::{App, Block, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

fn zsh() -> App {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/zsh",
        &["-f", "-i"],
        1,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("zsh starts: {e}"));
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "zsh reaches its prompt and the box takes the keys"
    );
    app
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

/// The rows of the live grid: what the program was told its window is.
fn grid_rows(app: &App) -> usize {
    match app.session().document().blocks().last() {
        Some(Block::Grid(g)) => g.size().1,
        _ => 0,
    }
}

/// Wait until `done` holds, taking the program's output as it arrives; each
/// wait blocks on output, nothing wakes on a clock.
fn until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
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

#[test]
fn with_the_box_off_keys_reach_the_shell_as_typed_and_the_program_gets_the_rows() {
    let mut app = zsh();
    let with_box = grid_rows(&app);
    assert!(app.set_input_box(false), "the window draws again");
    assert_eq!(app.input_mode(), Mode::Raw, "the box takes no keys");
    assert!(
        until(&mut app, |a| grid_rows(a)
            == with_box + EDITOR_ROWS as usize),
        "the program is told it has the box's rows: {} then {}",
        with_box,
        grid_rows(&app)
    );

    type_str(&mut app, "echo inline-$((40+2))");
    assert_eq!(app.input_text(), "", "nothing waits in a box");
    assert!(
        until(&mut app, |a| grid_text(a).contains("echo inline-$((40+2))")),
        "the shell has the keys before Enter and echoes them on its prompt line"
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("inline-42")),
        "Enter runs the line the shell holds"
    );
    assert_eq!(app.input_mode(), Mode::Raw);
}

#[test]
fn turning_the_box_off_keeps_a_line_already_in_it_until_its_enter() {
    let mut app = zsh();
    type_str(&mut app, "echo kept-$((1+1))");
    app.set_input_box(false);
    assert_eq!(app.input_text(), "echo kept-$((1+1))", "the line is kept");
    assert_eq!(app.input_mode(), Mode::Editing, "the box still edits it");
    type_str(&mut app, "3");
    app.pump();
    assert!(
        !grid_text(&app).contains("kept"),
        "nothing reached the shell before the box's Enter"
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("kept-23")),
        "the box's Enter sends the line whole"
    );
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Raw),
        "at the next prompt the box is off"
    );
}

#[test]
fn turned_back_on_the_box_takes_the_keys_at_the_next_prompt() {
    let mut app = zsh();
    app.set_input_box(false);
    // A program the shell starts, not a builtin: the tty shows the shell leave
    // its prompt and come back, which is what tells the box the line was read.
    type_str(&mut app, "/bin/echo off-$((2+2))");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("off-4")));
    assert!(app.set_input_box(true), "the box comes back");
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "the box takes the keys once the shell is at its prompt again"
    );
    type_str(&mut app, "echo on-$((3+3))");
    assert_eq!(app.input_text(), "echo on-$((3+3))");
    app.pump();
    assert!(
        !grid_text(&app).contains("echo on-"),
        "nothing sent before Enter"
    );
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("on-6")));
}
