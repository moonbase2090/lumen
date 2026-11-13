//! SGR mouse reports (DECSET 1006) from a grid block to a real program on a
//! PTY: what it receives is read back from what it prints.

#![cfg(unix)]

use lumen_term::mouse::{Button, Held, Wheel};
use lumen_term::{App, Block, GUTTER_COLUMNS, HEADER_ROWS, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

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

fn pump_until(app: &mut App, done: impl Fn(&str) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        app.pump();
        if done(&text(app)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A program that switches on `modes`, prints READY, then shows every byte it
/// is sent (escape as `^[`) until it ends.
fn start(modes: &str) -> App {
    let script = format!("printf '\\033[?{modes}h'; stty raw -echo; printf READY; exec cat -v");
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        WIDTH,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("READY")), "program up");
    app
}

/// The pixel in the middle of cell `(col, row)` of the live block.
fn at(app: &App, col: u32, row: u32) -> (u32, u32) {
    let (cw, ch) = (8, app.cell_height());
    (
        (GUTTER_COLUMNS + col) * cw + 1,
        (HEADER_ROWS + row) * ch + 1,
    )
}

/// Type `mark`, which `cat -v` shows after everything sent before it.
fn mark(app: &mut App, mark: char) {
    app.key(Key::Char(mark), Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(pump_until(app, |t| t.contains(mark)), "mark echoed");
}

const NONE: Held = Held {
    shift: false,
    alt: false,
    ctrl: false,
};

#[test]
fn press_release_drag_and_wheel_are_reported_in_grid_cells() {
    let mut app = start("1002;1006");
    let (x, y) = at(&app, 4, 2);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    let (x2, y2) = at(&app, 9, 3);
    assert!(
        app.mouse_move(x2, y2, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !app.mouse_move(x2 + 1, y2, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}")),
        "same cell: quiet"
    );
    assert!(
        app.mouse_release(x2, y2, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_wheel(x, y, Wheel::Up, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_wheel(x, y, Wheel::Down, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    let seen = text(&app);
    assert!(
        seen.contains("READY^[[<0;5;3M^[[<32;10;4M^[[<0;10;4m^[[<64;5;3M^[[<65;5;3MZ"),
        "reports: {seen}"
    );
}

#[test]
fn other_buttons_and_modifiers_are_in_the_code() {
    let mut app = start("1000;1006");
    let (x, y) = at(&app, 0, 0);
    let ctrl = Held { ctrl: true, ..NONE };
    assert!(
        app.mouse_press(x, y, Button::Right, ctrl)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_release(x, y, Button::Right, ctrl)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_press(x, y, Button::Middle, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    let seen = text(&app);
    assert!(seen.contains("^[[<18;1;1M^[[<18;1;1m^[[<1;1;1MZ"), "{seen}");
}

#[test]
fn level_1000_reports_no_drag_and_level_1003_reports_bare_motion() {
    let mut app = start("1000;1006");
    let (x, y) = at(&app, 1, 1);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    let (x2, y2) = at(&app, 2, 1);
    assert!(
        !app.mouse_move(x2, y2, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    assert!(!text(&app).contains("^[[<32"), "{}", text(&app));

    let mut any = start("1003;1006");
    assert!(
        any.mouse_move(x, y, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut any, 'Z');
    assert!(text(&any).contains("^[[<35;2;2MZ"), "{}", text(&any));
}

#[test]
fn nothing_is_sent_off_the_block_or_without_the_encoding() {
    let mut app = start("1002;1006");
    // Over the gutter and over the header: the terminal's own margin.
    assert!(
        !app.mouse_press(2, 40, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !app.mouse_press(100, 2, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    // Below the block area, over the editor.
    assert!(
        !app.mouse_wheel(100, HEIGHT - 5, Wheel::Up, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    assert!(!text(&app).contains("^[[<"), "{}", text(&app));

    // Tracking on, but the program did not ask for SGR: no other encoding.
    let mut plain = start("1002");
    let (x, y) = at(&plain, 3, 3);
    assert!(
        !plain
            .mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !plain
            .mouse_wheel(x, y, Wheel::Up, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut plain, 'Z');
    assert!(!text(&plain).contains("^[[<"), "{}", text(&plain));
}

#[test]
fn nothing_is_sent_when_the_mode_is_off() {
    // No mode ever on.
    let mut off = start("2004");
    let (x, y) = at(&off, 3, 3);
    assert!(
        !off.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !off.mouse_release(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !off.mouse_wheel(x, y, Wheel::Down, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut off, 'Z');
    assert!(!text(&off).contains("^[[<"), "{}", text(&off));

    // On, then turned off by the program: the reports stop.
    let script = "printf '\\033[?1002;1006h'; stty raw -echo; printf READY; \
                  dd bs=1 count=1 2>/dev/null; printf '\\033[?1002;1006l'; printf OFF; exec cat -v";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        WIDTH,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("READY")));
    let (x, y) = at(&app, 3, 3);
    // The program wants one byte first; a key does for it.
    app.key(Key::Char('q'), Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("OFF")));
    assert!(
        !app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !app.mouse_wheel(x, y, Wheel::Up, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    assert!(!text(&app).contains("^[[<"), "{}", text(&app));
}

#[test]
fn a_release_is_not_sent_for_a_press_that_was_not() {
    let script = "stty raw -echo; printf READY; dd bs=1 count=1 2>/dev/null; \
                  printf '\\033[?1002;1006h'; printf ON; exec cat -v";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        WIDTH,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("READY")));
    let (x, y) = at(&app, 3, 3);
    // Pressed while the mode is off, released after the program turned it on.
    assert!(
        !app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    app.key(Key::Char('q'), Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("ON")));
    assert!(
        !app.mouse_release(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark(&mut app, 'Z');
    assert!(!text(&app).contains("^[[<"), "{}", text(&app));
}
