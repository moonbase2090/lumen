//! Text an input method is composing is drawn where it will go: inline at the caret of the
//! input box, or at the program's cursor, underlined, with the candidate window anchored on it.
//! Real programs on a real PTY; the window's frame is read pixel by pixel.

#![cfg(unix)]

use lumen_term::input_line::Mode;
use lumen_term::raster::GUTTER_PAD_PX;
use lumen_term::{App, Block, Frame, Key, Mods, Passthrough, Renderer};
use std::time::{Duration, Instant};

const WIDTH: u32 = 800;

fn start(program: &str, args: &[&str]) -> App {
    App::start(Box::new(Passthrough), program, args, 1, WIDTH, 600)
        .unwrap_or_else(|e| unreachable!("{program} starts: {e}"))
}

fn renderer() -> Renderer {
    Renderer::new(1, lumen_term::design::theme())
        .unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
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

fn zsh() -> App {
    let mut app = start("/bin/zsh", &["-f", "-i"]);
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "zsh reaches its prompt and the box takes the keys"
    );
    app
}

fn raw_cat_after_text() -> App {
    let mut app = start("/bin/sh", &["-c", "printf 'ab'; exec cat"]);
    assert!(until(&mut app, |a| grid_text(a).contains("ab")
        && a.input_mode() == Mode::Raw));
    app
}

fn cell_w() -> u32 {
    renderer().cell().0
}

/// Pixels of `frame` in the rectangle that are not `colour`.
fn other_than(frame: &Frame, (x, y, w, h): (u32, u32, u32, u32), colour: [u8; 3]) -> usize {
    (y..y + h)
        .flat_map(|yy| (x..x + w).map(move |xx| (xx, yy)))
        .filter(|&(xx, yy)| frame.pixel(xx, yy) != Some(colour))
        .count()
}

/// Whether every pixel of row `y` from `x` for `w` pixels is `colour`.
fn row_is(frame: &Frame, (x, y, w): (u32, u32, u32), colour: [u8; 3]) -> bool {
    (x..x + w).all(|xx| frame.pixel(xx, y) == Some(colour))
}

#[test]
fn composing_text_is_drawn_in_the_box_underlined_where_the_candidate_window_is() {
    let theme = renderer().theme().clone();
    let mut app = zsh();
    type_str(&mut app, "ab");
    let plain = app.view();
    app.set_preedit("你好", Some((6, 6)));
    let (x, y, w, h) = app.ime_area();
    assert_eq!(w, 4 * cell_w(), "two wide characters are four columns");
    let composing = app.view();
    assert_ne!(composing, plain, "the composing text is drawn");
    assert!(
        row_is(&composing, (x, y + h - 1, w), theme.prompt),
        "an underline runs under all of it"
    );
    assert_ne!(
        composing.pixel(x + w + 4, y + h - 1),
        Some(theme.prompt),
        "and stops where the text stops"
    );
    for col in 0..w {
        assert_eq!(
            composing.pixel(x + col, y),
            plain.pixel(x + col, y),
            "the box's top border runs on over the composing text"
        );
    }
    assert_eq!(app.input_text(), "ab", "the box's own text is untouched");
}

#[test]
fn composing_letters_are_inked_in_the_text_colour_on_the_editor_background() {
    let theme = renderer().theme().clone();
    let mut app = zsh();
    app.set_preedit("nihao", None);
    let (x, y, w, h) = app.ime_area();
    assert_eq!(w, 5 * cell_w());
    let frame = app.view();
    assert!(
        other_than(&frame, (x, y, w, h - 2), theme.editor) > 100,
        "the letters are drawn above the underline"
    );
    assert_eq!(app.input_text(), "");
}

#[test]
fn the_rest_of_the_line_moves_right_to_make_room() {
    let mut app = zsh();
    type_str(&mut app, "abcd");
    for _ in 0..2 {
        app.key(Key::Left, Mods::default())
            .unwrap_or_else(|e| unreachable!("key: {e}"));
    }
    let plain = app.view();
    app.set_preedit("你好", None);
    let (x, y, w, h) = app.ime_area();
    let composing = app.view();
    let cw = cell_w();
    for row in 0..h {
        for col in 3..2 * cw {
            let before = plain.pixel(x + col, y + row);
            let after = composing.pixel(x + w + col, y + row);
            assert_eq!(after, before, "'cd' is drawn after the composing text");
        }
    }
    for row in 0..h - 2 {
        for col in 0..x {
            assert_eq!(
                composing.pixel(col, y + row),
                plain.pixel(col, y + row),
                "the text before the caret does not move"
            );
        }
    }
}

#[test]
fn clearing_or_committing_removes_the_composing_text() {
    let mut app = zsh();
    let empty = app.view();
    app.set_preedit("かな", Some((0, 0)));
    assert_ne!(app.view(), empty);
    app.clear_preedit();
    assert_eq!(app.view(), empty, "cancelled: the box is as it was");
    app.set_preedit("かな", Some((0, 0)));
    app.commit("かな")
        .unwrap_or_else(|e| unreachable!("the box takes it: {e}"));
    assert_eq!(app.preedit(), None);
    assert_eq!(app.input_text(), "かな", "the committed text is in the box");
    let committed = app.view();
    let mut fresh = zsh();
    fresh
        .commit("かな")
        .unwrap_or_else(|e| unreachable!("the box takes it: {e}"));
    assert_eq!(
        committed,
        fresh.view(),
        "committed, it looks as if it had been typed"
    );
}

#[test]
fn an_unfocused_window_draws_the_composing_text_without_the_caret_colour() {
    let theme = renderer().theme().clone();
    let mut app = zsh();
    app.set_preedit("あ", Some((0, 0)));
    let focused = app.view();
    app.set_focused(false);
    let (x, y, w, h) = app.ime_area();
    let blurred = app.view();
    assert!(row_is(&focused, (x, y + h - 1, w), theme.prompt));
    assert!(
        !row_is(&blurred, (x, y + h - 1, w), theme.prompt),
        "unfocused, the underline is muted"
    );
}

#[test]
fn a_long_preedit_is_clipped_to_the_box_and_the_window_keeps_its_size() {
    let mut app = zsh();
    let plain = app.view();
    app.set_preedit(&"あ".repeat(200), Some((0, 0)));
    let composing = app.view();
    assert_eq!(
        (composing.width, composing.height),
        (plain.width, plain.height)
    );
    let (_, y, _, h) = app.ime_area();
    let border = WIDTH - GUTTER_PAD_PX - 1;
    for row in 0..h {
        for col in border..WIDTH {
            assert_eq!(
                composing.pixel(col, y + row),
                plain.pixel(col, y + row),
                "the box's right border and what is past it are untouched"
            );
        }
    }
    assert_ne!(composing, plain, "what fits is drawn");
}

#[test]
fn a_selected_clause_is_underlined_thicker_than_the_rest() {
    let theme = renderer().theme().clone();
    let mut app = zsh();
    app.set_preedit("ab", Some((0, 1)));
    let (x, y, _, h) = app.ime_area();
    let frame = app.view();
    let cw = cell_w();
    assert!(
        row_is(&frame, (x, y + h - 2, cw), theme.prompt),
        "two rows under 'a'"
    );
    assert_ne!(
        frame.pixel(x + cw, y + h - 2),
        Some(theme.prompt),
        "one row under 'b'"
    );
    assert_eq!(frame.pixel(x + cw, y + h - 1), Some(theme.prompt));
}

#[test]
fn while_the_program_has_the_keys_the_composing_text_is_drawn_at_its_cursor() {
    let theme = renderer().theme().clone();
    let mut app = raw_cat_after_text();
    let plain = app.view();
    app.set_preedit("你", Some((3, 3)));
    let (x, y, w, h) = app.ime_area();
    assert_eq!(
        plain.pixel(x + 1, y + 1),
        Some(theme.foreground),
        "the candidate window is anchored on the cursor's cell"
    );
    let composing = app.view();
    assert!(row_is(&composing, (x, y + h - 1, w), theme.prompt));
    assert_eq!(
        composing.pixel(x + w + 4, y + 1),
        plain.pixel(x + w + 4, y + 1),
        "the rest of the program's screen is where it was"
    );
    assert_eq!(app.input_text(), "", "the box is not involved");
    app.clear_preedit();
    assert_eq!(
        app.view(),
        plain,
        "cancelled: the program's screen is as it was"
    );
}
