//! The cursor: block, bar and underline, blink, the unfocused dimming and show/hide, drawn
//! from what a program wrote into a grid block.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel counts of one cell are a few hundred"
)]

use lumen_term::raster::CursorState;
use lumen_term::{Block, Context, Document, Frame, GridBlock, Passthrough, Renderer};

const FOCUSED_ON: CursorState = CursorState {
    focused: true,
    blink_on: true,
};
const FOCUSED_OFF: CursorState = CursorState {
    focused: true,
    blink_on: false,
};
const UNFOCUSED: CursorState = CursorState {
    focused: false,
    blink_on: true,
};

fn renderer() -> Renderer {
    Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

/// A 10 by 3 block that has been given `bytes`, painted with the cursor in `state`.
fn painted(bytes: &[u8], state: Option<CursorState>) -> (Frame, Renderer) {
    let r = renderer();
    let mut block = GridBlock::new(10, 3);
    block.feed(bytes);
    let (w, h) = r.cell();
    let mut frame = Frame::new(10 * w, 3 * h, r.theme().background);
    r.paint_grid(&mut frame, 0, 0, &block);
    if let Some(state) = state {
        r.paint_cursor(&mut frame, 0, 0, &block, state);
    }
    (frame, r)
}

/// Pixels of the cell at `col`, `row` that are not the background.
fn inked(frame: &Frame, r: &Renderer, col: u32, row: u32) -> Vec<(u32, u32)> {
    let (w, h) = r.cell();
    let bg = Some(r.theme().background);
    let mut out = Vec::new();
    for y in row * h..(row + 1) * h {
        for x in col * w..(col + 1) * w {
            if frame.pixel(x, y) != bg {
                out.push((x - col * w, y - row * h));
            }
        }
    }
    out
}

#[test]
fn the_default_cursor_is_a_filled_block_on_the_cell_after_the_text() {
    let (with, r) = painted(b"ab", Some(FOCUSED_ON));
    let (w, h) = r.cell();
    let cell = inked(&with, &r, 2, 0);
    assert!(
        cell.len() as u32 > (w * h) * 9 / 10,
        "a block fills its cell"
    );
    assert!(inked(&with, &r, 3, 0).is_empty(), "and no other cell");
    assert!(
        !inked(&with, &r, 1, 0).is_empty(),
        "the text is still there"
    );
}

#[test]
fn a_block_over_a_letter_shows_the_letter_in_the_background_colour() {
    let (with, r) = painted(b"W\x1b[D", Some(FOCUSED_ON)); // cursor back onto the W
    let (without, _) = painted(b"W\x1b[D", None);
    let ink = Some(r.theme().foreground);
    let bg = Some(r.theme().background);
    let (w, h) = r.cell();
    let cell = || (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
    let letter = cell().filter(|&(x, y)| with.pixel(x, y) != ink).count();
    assert!(letter > 0, "the glyph is cut out of the block");
    assert!(
        cell().any(|(x, y)| with.pixel(x, y) == bg),
        "in the background colour"
    );
    assert!(
        cell().filter(|&(x, y)| with.pixel(x, y) == ink).count() > letter,
        "most of it is block"
    );
    assert_ne!(with, without);
}

#[test]
fn a_bar_is_a_thin_vertical_line_at_the_left_of_the_cell() {
    let (frame, r) = painted(b"\x1b[6 q", Some(FOCUSED_ON)); // steady bar
    let (w, h) = r.cell();
    let pixels = inked(&frame, &r, 0, 0);
    assert_ne!(pixels.len(), 0);
    assert!(
        pixels.iter().all(|(x, _)| *x < 2),
        "within two pixels of the left edge"
    );
    assert_eq!(
        pixels.len() as u32,
        2 * h,
        "two pixels wide, the cell's height"
    );
    assert!(w > 2);
}

#[test]
fn an_underline_is_a_thin_line_along_the_bottom_of_the_cell() {
    let (frame, r) = painted(b"\x1b[4 q", Some(FOCUSED_ON)); // steady underline
    let (w, h) = r.cell();
    let pixels = inked(&frame, &r, 0, 0);
    assert_eq!(
        pixels.len() as u32,
        2 * w,
        "two pixels high, the cell's width"
    );
    assert!(pixels.iter().all(|(_, y)| *y >= h - 2));
}

#[test]
fn the_cursor_follows_the_scale() {
    let r = Renderer::light(2).unwrap_or_else(|e| unreachable!("{e}"));
    let mut block = GridBlock::new(10, 3);
    block.feed(b"\x1b[6 q");
    let (w, h) = r.cell();
    let mut frame = Frame::new(10 * w, 3 * h, r.theme().background);
    r.paint_cursor(&mut frame, 0, 0, &block, FOCUSED_ON);
    assert_eq!(
        inked(&frame, &r, 0, 0).len() as u32,
        4 * h,
        "four pixels at scale 2"
    );
}

#[test]
fn a_hidden_cursor_draws_nothing_and_it_comes_back() {
    let (hidden, _) = painted(b"ab\x1b[?25l", Some(FOCUSED_ON));
    let (plain, _) = painted(b"ab\x1b[?25l", None);
    assert_eq!(hidden, plain, "DECTCEM off hides it");
    let (shown, _) = painted(b"ab\x1b[?25l\x1b[?25h", Some(FOCUSED_ON));
    assert_ne!(shown, plain, "DECTCEM on shows it again");
}

#[test]
fn a_blinking_cursor_is_drawn_in_its_on_half_only() {
    let bytes = b"ab\x1b[1 q"; // blinking block
    let (on, _) = painted(bytes, Some(FOCUSED_ON));
    let (off, _) = painted(bytes, Some(FOCUSED_OFF));
    let (none, _) = painted(bytes, None);
    assert_ne!(on, none);
    assert_eq!(off, none, "the off half shows no cursor");
}

#[test]
fn a_steady_cursor_ignores_the_blink_half() {
    let bytes = b"ab\x1b[2 q"; // steady block
    let (on, _) = painted(bytes, Some(FOCUSED_ON));
    let (off, _) = painted(bytes, Some(FOCUSED_OFF));
    assert_eq!(on, off);
}

#[test]
fn an_unfocused_block_is_an_outline_and_does_not_blink() {
    let (focused, r) = painted(b"ab", Some(FOCUSED_ON));
    let (dim, _) = painted(b"ab", Some(UNFOCUSED));
    let (w, h) = r.cell();
    let outline = inked(&dim, &r, 2, 0);
    assert_ne!(focused, dim);
    assert!((outline.len() as u32) < w * h / 2, "an outline, not a fill");
    assert!(
        outline.contains(&(0, 0)) && outline.contains(&(w - 1, h - 1)),
        "along the edge"
    );
    assert!(!outline.contains(&(w / 2, h / 2)), "hollow");
    let (blinking_off, _) = painted(
        b"ab\x1b[1 q",
        Some(CursorState {
            focused: false,
            blink_on: false,
        }),
    );
    let (blinking_on, _) = painted(b"ab\x1b[1 q", Some(UNFOCUSED));
    assert_eq!(
        blinking_off, blinking_on,
        "unfocused, a blinking cursor stays steady"
    );
    assert_ne!(blinking_on, painted(b"ab\x1b[1 q", None).0, "and visible");
}

#[test]
fn an_unfocused_bar_is_dimmer_than_a_focused_one() {
    let (focused, r) = painted(b"\x1b[6 q", Some(FOCUSED_ON));
    let (dim, _) = painted(b"\x1b[6 q", Some(UNFOCUSED));
    let at = |f: &Frame| f.pixel(0, 0).unwrap_or([0, 0, 0]);
    let bg = r.theme().background;
    let distance = |c: [u8; 3]| (0..3).map(|i| u32::from(c[i].abs_diff(bg[i]))).sum::<u32>();
    assert!(distance(at(&dim)) < distance(at(&focused)));
    assert_ne!(at(&dim), bg, "but still there");
}

#[test]
fn the_cursor_sits_on_the_last_cell_after_a_write_to_the_last_column() {
    let (frame, r) = painted(b"0123456789", Some(FOCUSED_ON));
    assert_ne!(inked(&frame, &r, 9, 0).len(), 0);
    let (plain, _) = painted(b"0123456789", None);
    assert_ne!(frame, plain);
}

#[test]
fn a_wide_character_under_the_cursor_gets_a_two_cell_block() {
    let (frame, r) = painted("\u{4e2d}\x1b[2D".as_bytes(), Some(FOCUSED_ON));
    let (w, h) = r.cell();
    let mut right = 0;
    for y in 0..h {
        for x in w..2 * w {
            if frame.pixel(x, y) == Some(r.theme().foreground) {
                right += 1;
            }
        }
    }
    assert!(right as u32 > w * h / 2, "the block covers both cells");
}

#[test]
fn only_the_live_grid_of_a_window_has_a_cursor_and_chrome_is_untouched() {
    use lumen_term::Window;
    let win = Window {
        cols: 40,
        rows: 12,
        editor_row: 9,
        editor_rows: 2,
    };
    let r = renderer();
    let mut doc = Document::new(36, 6);
    doc.receive(&mut Passthrough, b"hello", Context::default());
    assert!(matches!(doc.blocks(), [Block::Grid(_)]));
    let plain = r.render_window_with(&doc, &win, &[]);
    let with = r.render_window_cursor(&doc, &win, &[], Some(FOCUSED_ON));
    let none = r.render_window_cursor(&doc, &win, &[], None);
    assert_eq!(plain, none, "no cursor unless asked");
    assert_ne!(plain, with);
    let (_, h) = r.cell();
    // Every changed pixel lies in the content rows, above the editor.
    let (w_px, _) = (with.width, with.height);
    for y in 0..with.height {
        for x in 0..w_px {
            if plain.pixel(x, y) != with.pixel(x, y) {
                assert!(y < 9 * h, "the cursor stays out of the editor and below");
            }
        }
    }
    // An exited program has no cursor: an empty document draws none.
    let empty = Document::new(36, 6);
    assert_eq!(
        r.render_window_with(&empty, &win, &[]),
        r.render_window_cursor(&empty, &win, &[], Some(FOCUSED_ON))
    );
}
