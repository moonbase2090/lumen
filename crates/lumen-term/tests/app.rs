//! The window's logic, without a window.

#![cfg(unix)]

use lumen_term::app::{block_cells, cells_for, window_for};
use lumen_term::{App, Block, Frame, Key, Mods, Passthrough};
use lumen_term::{GUTTER_COLUMNS, HEADER_ROWS};
use std::time::{Duration, Instant};

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

fn start(script: &str, width: u32, height: u32) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        width,
        height,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

#[test]
fn pixels_make_cells_within_the_limits() {
    assert_eq!(cells_for(800, 600, (8, 19)), (100, 31));
    assert_eq!(
        cells_for(7, 18, (8, 19)),
        (1, 1),
        "a tiny window is one cell"
    );
    assert_eq!(cells_for(0, 0, (8, 19)), (1, 1));
    assert_eq!(cells_for(100_000, 100_000, (8, 19)), (1000, 500), "capped");
    assert_eq!(
        cells_for(80, 38, (0, 0)),
        (80, 38),
        "a zero cell does not divide by zero"
    );
}

/// An empty gutter is drawn two cells wide, then the 6 pixel padding.
const EMPTY_GUTTER_CELLS: u32 = 2;

#[test]
fn the_window_size_sets_the_ptys_size() {
    // 800 pixels: 22 for the empty gutter and its padding leave 97 cells.
    let mut app = start("stty size", 800, 600);
    assert!(pump_until(&mut app, |a| text(a).contains("27 97")));
}

#[test]
fn resizing_the_window_resizes_the_program() {
    let mut app = start("echo first; read x; stty size", 800, 600);
    assert!(pump_until(&mut app, |a| text(a).contains("first")));
    assert!(app.resize(400, 380).is_ok());
    let view = app.view();
    assert_eq!(
        (view.width, view.height),
        (400, 380),
        "the view follows the window"
    );
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("16 47")));
}

#[test]
fn keys_reach_the_program() {
    let mut app = start("read x; echo got:$x", 640, 380);
    for c in "hey".chars() {
        assert!(app.key(Key::Char(c), Mods::default()).is_ok());
    }
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("got:hey")));
}

#[test]
fn control_c_reaches_the_program_as_a_signal() {
    let mut app = start(
        "trap 'echo interrupted; exit 0' INT; echo ready; while :; do sleep 1; done",
        640,
        380,
    );
    assert!(pump_until(&mut app, |a| text(a).contains("ready")));
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert!(app.key(Key::Char('c'), ctrl).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("interrupted")));
}

#[test]
fn pasted_text_is_sent_as_it_is() {
    let mut app = start("read x; echo got:$x", 640, 380);
    assert!(app.text("pasted").is_ok());
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
    assert!(pump_until(&mut app, |a| text(a).contains("got:pasted")));
}

#[test]
fn the_view_is_exactly_the_window_size_and_shows_the_programs_text() {
    let mut app = start("echo shown", 640, 380);
    assert!(pump_until(&mut app, |a| text(a).contains("shown")));
    let view = app.view();
    assert_eq!((view.width, view.height), (640, 380));
    let (cell_w, cell_h) = (8, 19);
    let mut first_block_row = HEADER_ROWS * cell_h..(HEADER_ROWS + 1) * cell_h;
    let content_columns = GUTTER_COLUMNS * cell_w..640;
    assert!(
        first_block_row.any(|y| content_columns
            .clone()
            .any(|x| view.pixel(x, y) != Some([255, 255, 255]))),
        "ink in the first row of the block area"
    );
}

#[test]
fn the_program_exiting_is_seen() {
    let mut app = start("true", 320, 190);
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !app.has_exited() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.has_exited());
    assert!(app.has_exited(), "asking again still says so");
}

#[test]
fn a_viewport_shows_the_top_of_a_short_frame_and_the_bottom_of_a_tall_one() {
    let mut f = Frame::new(4, 6, [255, 255, 255]);
    for (y, v) in (0..6).zip([0u8, 10, 20, 30, 40, 50]) {
        f.fill(0, y, 4, 1, [v, 0, 0]);
    }
    let short = f.viewport(6, 8, [9, 9, 9]);
    assert_eq!((short.width, short.height), (6, 8));
    assert_eq!(short.pixel(0, 0), Some([0, 0, 0]), "top aligned");
    assert_eq!(short.pixel(0, 5), Some([50, 0, 0]));
    assert_eq!(short.pixel(0, 6), Some([9, 9, 9]), "below the frame");
    assert_eq!(short.pixel(5, 0), Some([9, 9, 9]), "right of the frame");
    let tall = f.viewport(4, 3, [9, 9, 9]);
    assert_eq!((tall.width, tall.height), (4, 3));
    assert_eq!(tall.pixel(0, 0), Some([30, 0, 0]), "the last three rows");
    assert_eq!(tall.pixel(0, 2), Some([50, 0, 0]));
    let narrow = f.viewport(2, 6, [9, 9, 9]);
    assert_eq!(narrow.pixel(1, 3), Some([30, 0, 0]), "cropped on the right");
}

#[test]
fn the_block_area_is_what_the_window_leaves_after_the_margin_the_editor_and_the_strip_row() {
    let win = window_for(100, 31);
    assert_eq!((win.cols, win.rows), (100, 31));
    assert_eq!((win.editor_row, win.editor_rows), (28, 2));
    assert_eq!(block_cells(100, 31), (96, 27));
    assert_eq!(block_cells(80, 24), (76, 20));
}

#[test]
fn a_window_too_small_for_the_margin_still_has_a_one_by_one_block_area() {
    for (c, r) in [(1, 1), (4, 4), (5, 5), (3, 40), (40, 3)] {
        let (bc, br) = block_cells(c, r);
        assert!(bc >= 1 && br >= 1, "{c}x{r}");
    }
    assert_eq!(block_cells(1, 1), (1, 1));
    let app = start("true", 8, 19);
    let view = app.view();
    assert_eq!((view.width, view.height), (8, 19));
    assert_eq!(app.session().document().columns(), 1);
    assert_eq!(app.session().document().rows(), 1);
}

#[test]
fn the_view_draws_the_trust_margin_the_producer_cannot_reach() {
    let Ok(r) = lumen_term::Renderer::new(1, lumen_term::design::theme()) else {
        unreachable!("the face loads")
    };
    let mut app = start("echo shown", 640, 380);
    assert!(pump_until(&mut app, |a| text(a).contains("shown")));
    for _ in 0..200 {
        if app.has_exited() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.has_exited());
    let view = app.view();
    let (cw, ch) = r.cell();
    let pad = r.gutter_pad();
    // The layout counts the gutter's four columns; the empty gutter is drawn in two.
    let gutter = EMPTY_GUTTER_CELLS * cw;
    let (cols, rows) = cells_for(640 - gutter - pad + GUTTER_COLUMNS * cw, 380, (cw, ch));
    let win = window_for(cols, rows);
    // The box shows the terminal's own status, never the program's text.
    let ended = lumen_term::raster::InputView {
        status: Some("The program has ended".to_owned()),
        focused: true,
        ..Default::default()
    };
    let expect = r.render_window_input(
        &lumen_term::Document::new(1, 1),
        &win,
        &[],
        None,
        lumen_term::scroll::Scroll::Follow,
        (None, None),
        Some(&ended),
    );
    let lay = lumen_term::layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
    for rect in lay.margin.iter().chain([&lay.editor]) {
        for y in rect.y * ch..(rect.y + rect.h) * ch {
            // Right of the gutter the text column starts past the drawn
            // gutter and its padding.
            let content = |col: u32| gutter + pad + (col - GUTTER_COLUMNS) * cw;
            let shift = |col: u32| {
                if col > GUTTER_COLUMNS {
                    content(col)
                } else {
                    (col * cw).min(gutter)
                }
            };
            let left = if rect.x >= GUTTER_COLUMNS {
                content(rect.x)
            } else {
                (rect.x * cw).min(gutter)
            };
            for x in left..shift(rect.x + rect.w) {
                assert_eq!(view.pixel(x, y), expect.pixel(x, y), "pixel {x},{y}");
            }
        }
    }
}

/// The text column starts a gutter padding right of the divider, at every
/// scale, and the input box starts at the same column.
#[test]
fn text_starts_clear_of_the_gutter_divider() {
    for scale in [1u32, 2] {
        let Ok(mut app) = App::start(
            Box::new(Passthrough),
            "/bin/sh",
            &["-c", "echo shown"],
            scale,
            640 * scale,
            380 * scale,
        ) else {
            unreachable!("the app starts")
        };
        assert!(pump_until(&mut app, |a| text(a).contains("shown")));
        let Ok(r) = lumen_term::Renderer::new(scale, lumen_term::design::theme()) else {
            unreachable!("the face loads")
        };
        let (cw, ch) = r.cell();
        let view = app.view();
        let divider_right = EMPTY_GUTTER_CELLS * cw;
        let pad = 6 * scale;
        let bg = lumen_term::design::theme().background;
        // The first row of content: no pixel of text left of the padded column.
        let row = HEADER_ROWS * ch;
        let first_ink = (divider_right..view.width)
            .find(|&x| (row..row + ch).any(|y| view.pixel(x, y).is_some_and(|c| c != bg)));
        let Some(first_ink) = first_ink else {
            unreachable!("the text is drawn")
        };
        assert!(
            first_ink >= divider_right + pad,
            "scale {scale}: text starts at {first_ink}, divider ends at {divider_right}"
        );
        assert!(
            first_ink < divider_right + pad + cw,
            "scale {scale}: text starts at {first_ink}, more than a cell past the padding"
        );
    }
}
