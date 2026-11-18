//! Mouse reports and bracketed paste go only to the foreground process group
//! of the program's terminal: a mode a program switched on does not follow the
//! terminal to whatever is in front of it later. A real shell with job control
//! puts each job in its own group and gives it the terminal.

#![cfg(unix)]

use lumen_term::app::PasteOutcome;
use lumen_term::mouse::{Button, Held};
use lumen_term::{App, Block, GUTTER_COLUMNS, HEADER_ROWS, Key, Mods, Passthrough};
use std::time::Duration;

const NONE: Held = Held {
    shift: false,
    alt: false,
    ctrl: false,
};

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

/// Take the program's output as it arrives until `done` holds. Each wait ends
/// when output comes; the bound only turns a program that went silent into a
/// failure.
fn pump_until(app: &mut App, done: impl Fn(&str) -> bool) -> bool {
    const SILENCE: Duration = Duration::from_secs(20);
    app.pump();
    while !done(&text(app)) {
        if app.pump_wait(SILENCE) == 0 {
            return done(&text(app));
        }
    }
    true
}

/// A job that shows every byte it is sent (escape as `^[`) and says `up`.
const JOB: &str = "stty raw -echo; echo up; exec cat -v";

/// Start a program that runs `script` in a shell. Returns once it has printed `waiting`.
fn start(script: &str) -> App {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(
        pump_until(&mut app, |t| t.contains("waiting") || t.contains("up")),
        "program up: {}",
        text(&app)
    );
    app
}

/// A shell switches `modes` on while it holds the terminal, waits for the test
/// to say go, then runs the job in its own process group, which takes the
/// terminal: the shell is no longer in front.
fn shell_then_job(modes: &str) -> App {
    let script = format!("printf '{modes}'; echo waiting; read go; set -m; sh -c '{JOB}'");
    let mut app = start(&script);
    // Let the app see the modes while the shell is in front, then let the job go.
    app.text("go\n")
        .unwrap_or_else(|e| unreachable!("text: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("up")), "the job is up");
    app
}

/// A job that switches `modes` on itself, in its own group and in front.
fn job_sets_modes(modes: &str) -> App {
    let script =
        format!("set -m; sh -c 'stty raw -echo; printf \"{modes}\"; echo up; exec cat -v'");
    start(&script)
}

fn at(app: &App, col: u32, row: u32) -> (u32, u32) {
    let (cw, ch) = (8, app.cell_height());
    (
        (GUTTER_COLUMNS + col) * cw + 6 + 1,
        (HEADER_ROWS + row) * ch + 1,
    )
}

/// Type `mark`, which `cat -v` shows after everything sent before it.
fn mark(app: &mut App, mark: char) {
    app.key(Key::Char(mark), Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(pump_until(app, |t| t.contains(mark)), "mark echoed");
}

const PASTE_ON: &str = "\\033[?2004h";
const MOUSE_ON: &str = "\\033[?1000h\\033[?1006h";

#[test]
fn a_paste_is_bracketed_when_the_program_in_front_asked_for_it() {
    let mut app = job_sets_modes(PASTE_ON);
    assert_eq!(app.paste("abcde").ok(), Some(PasteOutcome::Sent));
    mark(&mut app, 'Z');
    assert!(
        text(&app).contains("^[[200~abcde^[[201~Z"),
        "{}",
        text(&app)
    );
}

#[test]
fn a_paste_after_another_group_took_the_terminal_is_not_bracketed() {
    // The shell switched bracketed paste on and then a job took the terminal:
    // the job never asked for it, so no paste markers reach it.
    let mut app = shell_then_job(PASTE_ON);
    assert_eq!(app.paste("abcde").ok(), Some(PasteOutcome::Sent));
    mark(&mut app, 'Z');
    let screen = text(&app);
    assert!(screen.contains("abcdeZ"), "{screen}");
    assert!(!screen.contains("200~"), "{screen}");
}

#[test]
fn a_mouse_report_goes_to_the_program_in_front_that_asked_for_it() {
    let mut app = job_sets_modes(MOUSE_ON);
    let (x, y) = at(&app, 2, 3);
    assert!(app.mouse_press(x, y, Button::Right, NONE).unwrap_or(false));
    app.mouse_release(x, y, Button::Right, NONE).ok();
    mark(&mut app, 'Z');
    assert!(text(&app).contains("^[[<2;3;4M"), "{}", text(&app));
}

#[test]
fn no_mouse_report_goes_to_a_group_that_did_not_ask_for_it() {
    let mut app = shell_then_job(MOUSE_ON);
    let (x, y) = at(&app, 2, 3);
    assert!(
        !app.mouse_press(x, y, Button::Right, NONE).unwrap_or(true),
        "nothing was sent"
    );
    app.mouse_release(x, y, Button::Right, NONE).ok();
    mark(&mut app, 'Z');
    let screen = text(&app);
    assert!(!screen.contains("^[[<"), "{screen}");
    assert!(screen.contains('Z'));
}
