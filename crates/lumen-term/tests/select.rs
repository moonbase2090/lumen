//! Selecting and copying text: grid rows, the wrapped lines of a rich block,
//! and the pointer that does it in a real window's app.

#![cfg(unix)]

use lumen_detector::TAG;
use lumen_term::actions::Action;
use lumen_term::app::Outcome;
use lumen_term::clipboard::Clipboard;
use lumen_term::mouse::{Button, Held};
use lumen_term::select::{Pos, Selection, copy_selection, select_all};
use lumen_term::{App, Context, Document, Layer, Passthrough, Renderer};
use std::cell::RefCell;
use std::time::{Duration, Instant};

fn paragraph(text: &str) -> Vec<u8> {
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(0x80);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend(body);
    v
}

fn b_layer() -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut l);
    l
}

/// A document of a grid row, a rich paragraph and another grid row.
fn document(text: &str) -> Document {
    let mut doc = Document::new(60, 4);
    let mut layer = b_layer();
    let mut bytes = b"first row\r\n".to_vec();
    bytes.extend(paragraph(text));
    bytes.extend_from_slice(b"last row\r\n");
    doc.receive(&mut layer, &bytes, Context::default());
    doc
}

fn whole(lines: &[lumen_term::select::FlowLine]) -> Selection {
    select_all(lines).unwrap_or_else(|| unreachable!("there are lines"))
}

#[test]
fn a_selection_over_a_wrapped_paragraph_copies_it_as_the_one_line_it_was() {
    let text = "the quick brown fox jumps over the lazy dog and keeps running past the river bank";
    let doc = document(text);
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    // Narrow enough that the paragraph wraps onto several lines.
    let lines = r.flow(&doc, 30);
    let rich: Vec<_> = lines.iter().filter(|l| !l.grid).collect();
    assert!(rich.len() >= 3, "the paragraph wrapped: {rich:?}");
    let copied = copy_selection(&lines, &whole(&lines)).map(|c| c.text);
    let copied = copied.unwrap_or_default();
    assert!(
        copied.contains(text),
        "the wrapped lines come back as one: {copied:?}"
    );
    assert!(copied.starts_with("first row\n"), "{copied:?}");
    assert!(copied.contains("\nlast row"), "{copied:?}");
}

#[test]
fn select_all_does_not_copy_the_empty_rows_below_the_last_text() {
    let doc = document("a paragraph");
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let mut lines = r.flow(&doc, 60);
    let blank = lines
        .last()
        .cloned()
        .unwrap_or_else(|| unreachable!("lines"));
    for _ in 0..5 {
        let mut b = blank.clone();
        b.text = String::new();
        lines.push(b);
    }
    let copied = copy_selection(&lines, &whole(&lines))
        .map(|c| c.text)
        .unwrap_or_default();
    assert!(
        copied.ends_with("last row"),
        "no trailing blank lines: {copied:?}"
    );
}

#[test]
fn a_word_cut_by_the_wrap_is_joined_without_a_space() {
    let word = "x".repeat(70);
    let doc = document(&word);
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let lines = r.flow(&doc, 30);
    assert!(lines.iter().filter(|l| !l.grid).count() >= 3);
    let copied = copy_selection(&lines, &whole(&lines))
        .map(|c| c.text)
        .unwrap_or_default();
    assert!(copied.contains(&word), "{copied:?}");
}

#[test]
fn a_part_of_a_line_copies_only_that_part() {
    let doc = document("alpha beta");
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let lines = r.flow(&doc, 60);
    // "first row": characters 2..7 are "rst r".
    let sel = Selection {
        anchor: Pos { line: 0, col: 2 },
        head: Pos { line: 0, col: 7 },
    };
    assert_eq!(
        copy_selection(&lines, &sel).map(|c| c.text).as_deref(),
        Some("rst r")
    );
    // Dragging backwards gives the same text.
    let back = Selection {
        anchor: sel.head,
        head: sel.anchor,
    };
    assert_eq!(
        copy_selection(&lines, &back).map(|c| c.text).as_deref(),
        Some("rst r")
    );
    assert!(
        copy_selection(
            &lines,
            &Selection {
                anchor: sel.anchor,
                head: sel.anchor
            }
        )
        .is_none()
    );
}

#[test]
fn a_selection_across_a_grid_and_a_rich_block_keeps_the_line_ends() {
    let doc = document("alpha beta");
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let lines = r.flow(&doc, 60);
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    let first_rich = lines.iter().position(|l| !l.grid).unwrap_or(0);
    // From the middle of "first row" to the middle of "alpha beta".
    let sel = Selection {
        anchor: Pos { line: 0, col: 6 },
        head: Pos {
            line: first_rich,
            col: 5,
        },
    };
    assert_eq!(texts[0], "first row");
    let copied = copy_selection(&lines, &sel)
        .map(|c| c.text)
        .unwrap_or_default();
    assert!(copied.starts_with("row\n"), "{copied:?}");
    assert!(copied.ends_with("alpha"), "{copied:?}");
}

#[test]
fn the_hit_test_and_the_highlight_agree_with_the_layout() {
    let doc = document("alpha beta");
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let lines = r.flow(&doc, 60);
    let rich = lines.iter().find(|l| !l.grid).unwrap_or(&lines[0]);
    // Proportional text: each boundary is where the previous characters end,
    // and a point inside a character goes to its nearer boundary.
    let n = rich.text.chars().count();
    assert_eq!(r.char_at(rich, 0), 0);
    assert_eq!(r.char_at(rich, r.char_left(rich, n) + 50), n);
    for i in 0..=n {
        assert_eq!(r.char_at(rich, r.char_left(rich, i)), i, "boundary {i}");
    }
    assert!(
        r.char_left(rich, 1) != r.cell().0,
        "the rich text is proportional, not on the cell grid"
    );
}

struct Clip(RefCell<Option<String>>);

impl Clipboard for Clip {
    fn text(&self) -> Option<String> {
        self.0.borrow().clone()
    }
    fn set_text(&self, text: &str) -> bool {
        *self.0.borrow_mut() = Some(text.to_owned());
        true
    }
}

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
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

/// The pixel at the left edge of column `col` and the middle of row `row` of
/// the content.
fn at(col: u32, row: u32) -> (u32, u32) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let (cw, ch) = r.cell();
    (
        lumen_term::GUTTER_COLUMNS * cw + r.gutter_pad() + col * cw + 1,
        lumen_term::HEADER_ROWS * ch + row * ch + ch / 2,
    )
}

#[test]
fn dragging_across_text_selects_it_and_copy_puts_it_on_the_clipboard() {
    let mut app = start("echo hello world; echo second line; sleep 30");
    assert!(pump_until(&mut app, |a| {
        a.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == "second line"),
            lumen_term::Block::Rich(_) => false,
        })
    }));
    let held = Held::default();
    let clip = Clip(RefCell::new(None));
    assert_eq!(
        app.perform(Action::Copy, &clip),
        Outcome::Nothing,
        "nothing selected"
    );
    assert!(clip.0.borrow().is_none(), "nothing was copied");
    let (x0, y0) = at(6, 0);
    let (x1, y1) = at(6, 1);
    assert!(!app.mouse_press(x0, y0, Button::Left, held).unwrap_or(true));
    assert!(app.take_selection_redraw());
    assert!(!app.mouse_move(x1, y1, held).unwrap_or(true));
    assert!(app.take_selection_redraw(), "the window draws the tint");
    app.mouse_release(x1, y1, Button::Left, held).ok();
    assert!(app.has_selection());
    assert_eq!(app.selected_text().as_deref(), Some("world\nsecond"));
    assert_eq!(app.perform(Action::Copy, &clip), Outcome::Nothing);
    assert_eq!(clip.0.borrow().as_deref(), Some("world\nsecond"));
    // A press with no drag clears the selection.
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_release(x0, y0, Button::Left, held).ok();
    assert!(app.take_selection_redraw());
    assert!(!app.has_selection());
}

#[test]
fn the_selection_is_drawn_and_the_text_stays_readable() {
    let mut app = start("echo hello world; sleep 30");
    assert!(pump_until(&mut app, |a| {
        a.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == "hello world"),
            lumen_term::Block::Rich(_) => false,
        })
    }));
    let before = app.view();
    let held = Held::default();
    let (x0, y0) = at(0, 0);
    let (x1, _) = at(5, 0);
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_move(x1, y0, held).ok();
    app.mouse_release(x1, y0, Button::Left, held).ok();
    let after = app.view();
    assert_ne!(before, after, "the selected cells are tinted");
    // Only the five selected cells of that row changed.
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let (cw, ch) = r.cell();
    let (left, top) = at(0, 0);
    let (left, top) = (left - 1, top - ch / 2);
    for y in 0..after.height {
        for x in 0..after.width {
            let inside = x >= left && x < left + 5 * cw && y >= top && y < top + ch;
            if !inside {
                assert_eq!(
                    before.pixel(x, y),
                    after.pixel(x, y),
                    "({x},{y}) is outside the selection"
                );
            }
        }
    }
}

#[test]
fn select_all_takes_every_line_and_a_program_that_asked_for_the_mouse_gets_it_unless_shift_is_held()
{
    let mut app = start("printf '\\033[?1000h\\033[?1006h'; echo mouse mode on; sleep 30");
    assert!(pump_until(&mut app, |a| {
        a.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == "mouse mode on"),
            lumen_term::Block::Rich(_) => false,
        })
    }));
    let (x, y) = at(2, 0);
    let held = Held::default();
    // The program gets the press as a report, and nothing is selected.
    assert!(app.mouse_press(x, y, Button::Left, held).unwrap_or(false));
    app.mouse_release(x, y, Button::Left, held).ok();
    assert!(!app.has_selection(), "the program had the mouse");
    // With Shift the terminal selects.
    let shift = Held {
        shift: true,
        ..Held::default()
    };
    let (x2, _) = at(8, 0);
    app.mouse_press(x, y, Button::Left, shift).ok();
    app.mouse_move(x2, y, shift).ok();
    app.mouse_release(x2, y, Button::Left, shift).ok();
    assert_eq!(app.selected_text().as_deref(), Some("use mo"));
    // Select all takes the whole window's text.
    assert_eq!(
        app.perform(Action::SelectAll, &Clip(RefCell::new(None))),
        Outcome::Redraw
    );
    assert!(
        app.selected_text()
            .unwrap_or_default()
            .contains("mouse mode on")
    );
}

/// Cycle 0's background output, then a grid row, then cycle 1's background
/// output: the strip of cycle 0 is the older one, drawn before the grid row.
fn after_an_older_strip() -> Document {
    let mut doc = Document::new(60, 4);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    doc.receive(&mut layer, &paragraph("one"), Context::default());
    doc.receive(&mut layer, &paragraph("two"), Context::default());
    doc.receive(&mut layer, b"later row\r\n", Context::default());
    doc.begin_prompt_cycle(&mut layer);
    Layer::close_container(&mut layer);
    doc.receive(&mut layer, &paragraph("three"), Context::default());
    doc
}

fn later_top(doc: &Document) -> u32 {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let lines = r.flow(doc, 40);
    let Some(line) = lines.iter().find(|l| l.text.starts_with("later row")) else {
        unreachable!("the grid row is in the flow")
    };
    line.top
}

#[test]
fn a_line_after_a_collapsed_older_strip_is_selected_below_its_row() {
    let doc = after_an_older_strip();
    assert_eq!(doc.strips().len(), 2);
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let cell_h = r.cell().1;
    assert_eq!(later_top(&doc), cell_h, "one collapsed row above it");
}

#[test]
fn a_line_after_an_open_older_strip_is_selected_below_all_its_rows() {
    let mut doc = after_an_older_strip();
    assert!(doc.user_opens_strip(0));
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let cell_h = r.cell().1;
    let blocks = u32::try_from(doc.strips()[0].blocks.len()).unwrap_or(0);
    assert!(blocks >= 2);
    assert_eq!(later_top(&doc), (1 + blocks) * cell_h);
}

const COUNT: &str = "i=1; while [ $i -le 300 ]; do echo line$i; i=$((i+1)); done;";

#[test]
fn the_pointer_selects_what_is_in_view_when_the_view_is_scrolled_and_select_all_takes_the_history()
{
    let mut app = start(&format!("{COUNT} sleep 30"));
    assert!(pump_until(&mut app, |a| a
        .visible_text()
        .iter()
        .any(|l| l == "line300")));
    assert!(app.scroll_end(true), "scrolled to the oldest output");
    assert_eq!(
        app.visible_text().first().map(String::as_str),
        Some("line1")
    );
    let held = Held::default();
    let (x0, y0) = at(0, 1);
    let (x1, y1) = at(5, 2);
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_move(x1, y1, held).ok();
    app.mouse_release(x1, y1, Button::Left, held).ok();
    assert_eq!(app.selected_text().as_deref(), Some("line2\nline3"));
    // Scrolling does not move the selection off its text.
    assert!(app.scroll_rows(-3));
    assert_eq!(app.selected_text().as_deref(), Some("line2\nline3"));
    // Select all reaches the scrolled-away history too.
    let clip = Clip(RefCell::new(None));
    app.perform(Action::SelectAll, &clip);
    let all = app.selected_text().unwrap_or_default();
    assert!(
        all.starts_with("line1\nline2\n"),
        "{:?}",
        &all[..20.min(all.len())]
    );
    assert!(all.contains("line300"));
}

#[test]
fn a_finished_drag_asks_for_a_copy_unless_the_setting_is_off_and_a_click_or_select_all_does_not() {
    let mut app = start("echo hello world; echo second line; sleep 30");
    assert!(pump_until(&mut app, |a| {
        a.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == "second line"),
            lumen_term::Block::Rich(_) => false,
        })
    }));
    let held = Held::default();
    let (x0, y0) = at(0, 0);
    let (x1, _) = at(5, 0);
    // A click with no drag selects nothing, so there is nothing to copy.
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_release(x0, y0, Button::Left, held).ok();
    assert!(!app.take_copy_on_select());
    // A drag asks for the copy once, when the button is released, not before.
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_move(x1, y0, held).ok();
    assert!(!app.take_copy_on_select(), "still dragging");
    app.mouse_release(x1, y0, Button::Left, held).ok();
    assert!(app.take_copy_on_select());
    assert!(!app.take_copy_on_select(), "asked once");
    // The request goes through the explicit copy, so the clipboard gets the text.
    let clip = Clip(RefCell::new(None));
    assert_eq!(app.perform(Action::Copy, &clip), Outcome::Nothing);
    assert_eq!(clip.0.borrow().as_deref(), Some("hello"));
    // Select all is not a mouse selection.
    assert_eq!(app.perform(Action::SelectAll, &clip), Outcome::Redraw);
    assert!(!app.take_copy_on_select());
    // With the setting off a drag asks for nothing.
    app.set_copy_on_select(false);
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_move(x1, y0, held).ok();
    app.mouse_release(x1, y0, Button::Left, held).ok();
    assert!(app.has_selection());
    assert!(!app.take_copy_on_select());
}
