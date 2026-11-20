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
    // Block on the program's output; the deadline only bounds a hang.
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        if done(&text(app)) {
            return true;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        app.pump_wait(left);
    }
}

/// A program that switches on `modes`, prints READY, then shows every byte it
/// is sent (escape as `^[`) until it ends.
/// Run `script` as its own job: `set -m` gives it a process group of its own
/// in front of the terminal, so the program that switches the reporting modes
/// on is the one that keeps the terminal.
fn job(script: &str) -> String {
    format!("set -m; sh -c '{}'", script.replace('\'', "'\\''"))
}

fn start(modes: &str) -> App {
    let script = format!("printf '\\033[?{modes}h'; stty raw -echo; printf READY; cat -v");
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &job(&script)],
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
        // The text column starts the gutter padding (6 px at scale 1) right of the gutter.
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
fn nothing_is_sent_off_the_block() {
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
                  dd bs=1 count=1 2>/dev/null; printf '\\033[?1002;1006l'; printf OFF; cat -v";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &job(script)],
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
                  printf '\\033[?1002;1006h'; printf ON; cat -v";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &job(script)],
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

#[test]
fn tracking_without_sgr_reports_in_the_default_encoding() {
    let mut app = start("1002");
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
    // Code, column and row are each one byte, 32 plus the value; a release names no button.
    assert!(
        seen.contains("READY^[[M %#^[[M@*$^[[M#*$^[[M`%#^[[Ma%#Z"),
        "reports: {seen}"
    );
    assert!(!seen.contains("^[[<"), "{seen}");
}

#[test]
fn the_default_encoding_carries_the_modifiers_and_other_buttons() {
    let mut app = start("1000");
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
    // Right is 2 and Control adds 16: 50 is '2'; a release with Control is 3 + 16: 51 is '3'; middle is 1: 33 is '!'.
    assert!(
        text(&app).contains("READY^[[M2!!^[[M3!!^[[M!!!Z"),
        "{}",
        text(&app)
    );
}

/// Type `Z`, which the hex reader shows as 5a after everything sent before it.
fn mark_hex(app: &mut App) {
    app.key(Key::Char('Z'), Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(pump_until(app, |t| t.contains("5a")), "mark echoed");
}

/// A program that switches tracking 1002 on in a window of `columns` columns, then
/// shows each byte it is sent as two hex digits, which a byte of 255 needs.
fn hex_reader() -> App {
    let script = "printf '\\033[?1002h'; stty raw -echo; printf READY; \
        python3 -u -c 'import os\nwhile True:\n b=os.read(0,1)\n if not b: break\n print(\"%02x\"%b[0], end=\" \", flush=True)'";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &job(script)],
        1,
        2400,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |t| t.contains("READY")), "program up");
    app
}

#[test]
fn a_press_past_column_223_is_not_sent_and_neither_is_the_rest_of_that_click() {
    // Column 223 counting from 1 is the last one a byte can name: 255.
    let mut app = hex_reader();
    let (x, y) = at(&app, 222, 1);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_release(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark_hex(&mut app);
    assert!(
        text(&app).contains("READY1b 5b 4d 20 ff 22 1b 5b 4d 23 ff 22 5a"),
        "{}",
        text(&app)
    );
    // One column further cannot be named: nothing of the click is sent.
    let mut app = hex_reader();
    let (x, y) = at(&app, 223, 1);
    assert!(
        !app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        !app.mouse_release(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark_hex(&mut app);
    assert!(text(&app).contains("READY5a"), "{}", text(&app));
    assert!(!text(&app).contains("1b 5b 4d"), "{}", text(&app));
}

#[test]
fn a_drag_that_ends_past_column_223_still_ends_at_the_last_cell_it_can_name() {
    let mut app = hex_reader();
    let (x, y) = at(&app, 4, 1);
    let (far, _) = at(&app, 260, 1);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_release(far, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark_hex(&mut app);
    // The release is clamped to column 223 (255), so the program does not see a button left down.
    assert!(
        text(&app).contains("1b 5b 4d 20 25 22 1b 5b 4d 23 ff 22 5a"),
        "{}",
        text(&app)
    );
}

#[test]
fn a_drag_past_column_223_is_reported_at_the_last_cell_it_can_name_once() {
    let mut app = hex_reader();
    let (x, y) = at(&app, 4, 1);
    let (far, _) = at(&app, 260, 1);
    let (farther, _) = at(&app, 270, 1);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(
        app.mouse_move(far, y, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    // A second cell out there is the same cell to this encoding: nothing more is sent.
    let _ = app.mouse_move(farther, y, NONE);
    assert!(
        app.mouse_release(farther, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    mark_hex(&mut app);
    // Press at column 5, the drag clamped to column 223 (255) once, then the release.
    assert!(
        text(&app).contains("1b 5b 4d 20 25 22 1b 5b 4d 40 ff 22 1b 5b 4d 23 ff 22 5a"),
        "{}",
        text(&app)
    );
}

#[test]
fn the_default_encoding_is_none_for_a_value_no_byte_can_hold() {
    use lumen_term::mouse::legacy;
    assert_eq!(legacy(u32::MAX, (0, 0)), None);
    assert_eq!(legacy(0, (u32::MAX, 0)), None);
    assert_eq!(legacy(0, (0, u32::MAX)), None);
    assert_eq!(legacy(224, (0, 0)), None);
    assert_eq!(
        legacy(223, (222, 222)),
        Some(vec![0x1b, b'[', b'M', 255, 255, 255])
    );
}

#[test]
fn a_program_that_asked_for_tracking_without_sgr_owns_the_pointer_and_shift_still_selects() {
    let mut app = start("1000");
    let (x, y) = at(&app, 4, 2);
    assert!(
        app.mouse_press(x, y, Button::Left, NONE)
            .unwrap_or_else(|e| unreachable!("write: {e}"))
    );
    assert!(!app.is_selecting(), "the program has the press");
    let _ = app.mouse_release(x, y, Button::Left, NONE);
    let _ = app.mouse_press(
        x,
        y,
        Button::Left,
        Held {
            shift: true,
            ..NONE
        },
    );
    assert!(app.is_selecting(), "Shift gives the press to the terminal");
}
