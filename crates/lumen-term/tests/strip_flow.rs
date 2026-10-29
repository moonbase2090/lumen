//! The strip of an older prompt cycle, drawn where that cycle's output ended.

#![allow(clippy::many_single_char_names)]

use lumen_detector::TAG;
use lumen_term::{Context, Document, Frame, Layer, Rect, Renderer, Window, layout};

const WIN: Window = Window {
    cols: 60,
    rows: 20,
    editor_row: 15,
    editor_rows: 2,
};

fn renderer() -> Renderer {
    Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

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
    lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    })
}

/// Two cycles of background output: `first` words in cycle 0 and `second` in cycle 1.
fn two_cycles(first: &[&str], second: &[&str]) -> Document {
    let mut d = Document::new(56, 14);
    let mut l = b_layer();
    for t in first {
        d.receive(&mut l, &paragraph(t), Context::default());
    }
    d.begin_prompt_cycle(&mut l);
    Layer::close_container(&mut l);
    for t in second {
        d.receive(&mut l, &paragraph(t), Context::default());
    }
    d
}

fn changed(r: &Renderer, a: &Frame, b: &Frame, rect: &Rect) -> usize {
    let (cw, ch) = r.cell();
    let mut n = 0;
    for y in rect.y * ch..(rect.y + rect.h) * ch {
        for x in rect.x * cw..(rect.x + rect.w) * cw {
            if a.pixel(x, y) != b.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

fn lay(d: &Document) -> lumen_term::Layout {
    let last = d.strips().last().map(|s| (s.open, s.count()));
    layout(WIN.cols, WIN.rows, WIN.editor_row, WIN.editor_rows, last)
}

/// The first content row, where an older strip with no foreground above it sits.
fn first_row(l: &lumen_term::Layout) -> Rect {
    Rect {
        x: l.content.x,
        y: l.content.y,
        w: l.content.w,
        h: 1,
    }
}

#[test]
fn an_older_cycles_strip_is_one_collapsed_row_in_the_flow_with_only_a_count() {
    let r = renderer();
    let empty = Document::new(56, 14);
    let base = r.render_window(&empty, &WIN);
    let d = two_cycles(&["alpha", "beta"], &["gamma"]);
    assert_eq!(d.strips().len(), 2);
    let l = lay(&d);
    let f = r.render_window(&d, &WIN);
    assert!(
        changed(&r, &base, &f, &first_row(&l)) > 0,
        "the older strip is drawn in the flow"
    );
    // Only the count shows: other words, same count, same pixels.
    let same_count = two_cycles(&["zzzzzz", "completely different words"], &["gamma"]);
    let g = r.render_window(&same_count, &WIN);
    assert_eq!(changed(&r, &f, &g, &first_row(&l)), 0);
    let whole_content = Rect {
        x: l.content.x,
        y: l.content.y,
        w: l.content.w,
        h: l.editor.y - l.content.y,
    };
    assert_eq!(
        changed(&r, &f, &g, &whole_content),
        0,
        "no word the producer wrote is drawn anywhere in the content"
    );
    // One row for the whole cycle, however many blocks it holds: the row below is empty.
    let second_row = Rect {
        y: l.content.y + 1,
        ..first_row(&l)
    };
    assert_eq!(
        changed(&r, &base, &f, &second_row),
        0,
        "one row, not one per block"
    );
    // A different count changes the row.
    let three = two_cycles(&["a", "b", "c"], &["gamma"]);
    let h = r.render_window(&three, &WIN);
    assert!(changed(&r, &f, &h, &first_row(&l)) > 0);
}

#[test]
fn the_newest_cycles_strip_is_not_drawn_in_the_flow_but_below_the_editor() {
    let r = renderer();
    let only_new = two_cycles(&[], &["gamma"]);
    assert_eq!(only_new.strips().len(), 1);
    let l = lay(&only_new);
    let base = r.render_window(&Document::new(56, 14), &WIN);
    let f = r.render_window(&only_new, &WIN);
    assert_eq!(
        changed(&r, &base, &f, &first_row(&l)),
        0,
        "nothing in the flow"
    );
    let below = Rect {
        x: 0,
        y: l.editor.bottom(),
        w: WIN.cols,
        h: WIN.rows - l.editor.bottom(),
    };
    assert!(
        changed(&r, &base, &f, &below) > 0,
        "the newest strip is below the editor"
    );
}

#[test]
fn opening_an_older_strip_shows_a_row_per_block_and_only_the_user_opens_it() {
    let r = renderer();
    let mut d = two_cycles(&["alpha", "beta"], &["gamma"]);
    let l = lay(&d);
    let closed = r.render_window(&d, &WIN);
    assert!(
        d.strips().iter().all(|s| !s.open),
        "closed until the user acts"
    );
    assert!(d.user_opens_strip(0));
    let open = r.render_window(&d, &WIN);
    let flow = Rect {
        x: l.content.x,
        y: l.content.y,
        w: l.content.w,
        h: 3,
    };
    assert!(
        changed(&r, &closed, &open, &flow) > 0,
        "the open strip shows its blocks"
    );
    // Closing it puts the pixels back.
    d.user_closes_strip(0);
    assert_eq!(r.render_window(&d, &WIN), closed);
}

#[test]
fn an_older_strip_never_changes_the_margin_the_editor_or_the_newest_strip() {
    let r = renderer();
    let with_old = two_cycles(&["alpha", "beta"], &["gamma"]);
    let without = two_cycles(&[], &["gamma"]);
    let l = lay(&with_old);
    assert_eq!(
        l,
        lay(&without),
        "the layout does not depend on the older strip"
    );
    let a = r.render_window(&with_old, &WIN);
    let b = r.render_window(&without, &WIN);
    for m in &l.margin {
        assert_eq!(changed(&r, &a, &b, m), 0, "margin");
    }
    assert_eq!(changed(&r, &a, &b, &l.editor), 0, "editor");
    let below = Rect {
        x: 0,
        y: l.editor.bottom(),
        w: WIN.cols,
        h: WIN.rows - l.editor.bottom(),
    };
    assert_eq!(changed(&r, &a, &b, &below), 0, "newest strip");
    // Opening the older strip moves nothing outside the content area.
    let mut open = two_cycles(&["alpha", "beta"], &["gamma"]);
    assert!(open.user_opens_strip(0));
    let c = r.render_window(&open, &WIN);
    for m in &l.margin {
        assert_eq!(changed(&r, &a, &c, m), 0);
    }
    assert_eq!(changed(&r, &a, &c, &l.editor), 0);
    assert_eq!(changed(&r, &a, &c, &below), 0);
}

#[test]
fn a_strip_that_would_pass_the_editor_is_clipped_and_the_editor_stays() {
    let r = renderer();
    // Many cycles, each with background output, open: far more rows than fit.
    let mut d = Document::new(56, 14);
    let mut l = b_layer();
    for c in 0..30 {
        for t in 0..3 {
            d.receive(
                &mut l,
                &paragraph(&format!("c{c} b{t}")),
                Context::default(),
            );
        }
        d.begin_prompt_cycle(&mut l);
        Layer::close_container(&mut l);
        d.user_opens_strip(c);
    }
    let lay_ = lay(&d);
    let base = r.render_window(&Document::new(56, 14), &WIN);
    let f = r.render_window(&d, &WIN);
    for m in &lay_.margin {
        assert_eq!(changed(&r, &base, &f, m), 0, "margin");
    }
    assert_eq!(changed(&r, &base, &f, &lay_.editor), 0, "editor");
}
