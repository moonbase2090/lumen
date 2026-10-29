//! The window: the trust margin, the editor and the background strip, drawn
//! into the headless raster, and a miniature of the pixel oracle of 2.3: the
//! changed pixels in the margin and the editor, over adversarial Track 0, are
//! counted against a baseline with no producer input.

// Short names for frames, layers and rectangles keep the comparisons readable.
#![allow(clippy::many_single_char_names)]

use lumen_detector::TAG;
use lumen_term::{Context, Document, Frame, Layer, Rect, Renderer, StripPlacement, Window, layout};

const WIN: Window = Window {
    cols: 80,
    rows: 24,
    editor_row: 19,
    editor_rows: 2,
};

fn renderer() -> Renderer {
    Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn paragraph(text: &str) -> Vec<u8> {
    frame(0x80, &string(text))
}

fn heading(text: &str) -> Vec<u8> {
    let mut body = vec![1];
    body.extend(string(text));
    frame(0x81, &body)
}

fn code(text: &str) -> Vec<u8> {
    let mut body = string("rust");
    body.extend(string(text));
    frame(0x82, &body)
}

fn b_layer(container: bool) -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    if container {
        Layer::open_container(&mut l);
    }
    l
}

fn doc() -> Document {
    Document::new(76, 18)
}

fn px(renderer: &Renderer, rect: &Rect) -> (u32, u32, u32, u32) {
    let (cell_w, cell_h) = renderer.cell();
    (
        rect.x * cell_w,
        rect.y * cell_h,
        rect.w * cell_w,
        rect.h * cell_h,
    )
}

/// Pixels that differ between `left` and `right` inside `rect` (in cells).
fn changed(renderer: &Renderer, left: &Frame, right: &Frame, rect: &Rect) -> usize {
    let (x0, y0, width, height) = px(renderer, rect);
    let mut count = 0;
    for y in y0..y0 + height {
        for x in x0..x0 + width {
            if left.pixel(x, y) != right.pixel(x, y) {
                count += 1;
            }
        }
    }
    count
}

fn lay(strip: Option<(bool, usize)>) -> lumen_term::Layout {
    layout(WIN.cols, WIN.rows, WIN.editor_row, WIN.editor_rows, strip)
}

/// What a hostile or merely odd program might write as Track 0.
fn adversarial() -> Vec<(&'static str, Vec<u8>)> {
    let mut many = Vec::new();
    for i in 0..200 {
        many.extend(paragraph(&format!("line {i}")));
    }
    vec![
        (
            "badges",
            paragraph("\u{2714} verified \u{23fa} exit 0 \u{2713} [ok] exit status 0"),
        ),
        (
            "margin-lookalike",
            paragraph("lumen lumen lumen lumen lumen lumen lumen"),
        ),
        ("long", paragraph(&"x".repeat(4000))),
        (
            "bidi-and-zero-width",
            paragraph("a\u{202e}b\u{200b}c\u{2066}d"),
        ),
        (
            "wide-and-combining",
            paragraph("\u{4e2d}\u{6587}\u{4e2d}\u{6587} e\u{301}\u{301}\u{301}"),
        ),
        ("heading", heading("EXIT 0 VERIFIED")),
        (
            "code-with-bidi",
            code("fn main() {}\n\u{202e}// reversed\nlet x = 1;"),
        ),
        ("many-blocks", many),
        (
            "cursor-moves-in-the-grid",
            b"\x1b[H\x1b[2Jlumen\x1b[1;1Hlumen\x1b[24;1Hlumen\x1b[2K".to_vec(),
        ),
        (
            "rejected-header-text",
            b"\x1b_BADTAG00\x80\x00\x00\x00\x04\x00\x02hi".to_vec(),
        ),
    ]
}

fn render_b(bytes: &[u8], container: bool) -> (Document, Frame) {
    let r = renderer();
    let mut d = doc();
    let mut l = b_layer(container);
    d.receive(&mut l, bytes, Context::default());
    let f = r.render_window(&d, &WIN);
    (d, f)
}

fn baseline() -> Frame {
    renderer().render_window(&doc(), &WIN)
}

#[test]
fn the_window_has_the_size_of_its_cells_and_the_baseline_is_stable() {
    let r = renderer();
    let (cw, ch) = r.cell();
    let a = baseline();
    assert_eq!((a.width, a.height), (80 * cw, 24 * ch));
    assert_eq!(a, baseline(), "the baseline taken twice must match exactly");
}

#[test]
fn no_producer_output_changes_a_margin_or_editor_pixel() {
    let r = renderer();
    let base = baseline();
    let l = lay(None);
    let mut runs = 0;
    for (name, bytes) in adversarial() {
        for container in [true, false] {
            let (_, f) = render_b(&bytes, container);
            for m in &l.margin {
                assert_eq!(
                    changed(&r, &base, &f, m),
                    0,
                    "{name} container={container}: margin"
                );
            }
            assert_eq!(
                changed(&r, &base, &f, &l.editor),
                0,
                "{name} container={container}: editor"
            );
            runs += 1;
        }
    }
    assert_eq!(runs, 20);
}

#[test]
fn the_margin_is_the_terminals_colour_with_its_own_text() {
    let r = renderer();
    let f = baseline();
    let l = lay(None);
    let (gx, gy, gw, gh) = px(&r, &l.margin[0]);
    let colour = r.theme().margin;
    for y in gy..gy + gh {
        for x in gx..gx + gw {
            assert_eq!(f.pixel(x, y), Some(colour), "gutter pixel {x},{y}");
        }
    }
    let (hx, hy, hw, hh) = px(&r, &l.margin[1]);
    let text =
        (hy..hy + hh).any(|y| (hx..hx + hw).any(|x| f.pixel(x, y) == Some(r.theme().margin_text)));
    assert!(text, "the header carries the terminal's own label");
}

#[test]
fn background_output_appears_in_the_strip_and_nowhere_else() {
    let r = renderer();
    let base = baseline();
    let mut bytes = paragraph("one");
    bytes.extend(paragraph("two"));
    let (d, f) = render_b(&bytes, false);
    assert_eq!(d.strips()[0].count(), 2);
    let l = lay(Some((false, 2)));
    let StripPlacement::Placed(strip) = l.strip else {
        unreachable!("there is room")
    };
    assert!(changed(&r, &base, &f, &strip) > 0, "the strip is drawn");
    let everything = Rect {
        x: 0,
        y: 0,
        w: WIN.cols,
        h: WIN.rows,
    };
    let outside = changed(&r, &base, &f, &everything) - changed(&r, &base, &f, &strip);
    assert_eq!(outside, 0, "no pixel outside the strip changed");
}

#[test]
fn a_collapsed_strip_shows_only_the_count_not_what_the_producer_wrote() {
    let r = renderer();
    let l = lay(Some((false, 2)));
    let StripPlacement::Placed(strip) = l.strip else {
        unreachable!("there is room")
    };
    let mut one = paragraph("alpha");
    one.extend(paragraph("beta"));
    let mut two = paragraph("zzzzzzzz");
    two.extend(paragraph("completely different words"));
    let (_, a) = render_b(&one, false);
    let (_, b) = render_b(&two, false);
    assert_eq!(changed(&r, &a, &b, &strip), 0, "same count, same pixels");
    let (_, c) = render_b(&paragraph("alpha"), false);
    assert!(
        changed(&r, &a, &c, &strip) > 0,
        "the count changes the label"
    );
}

#[test]
fn opening_the_strip_changes_the_strip_and_nothing_else() {
    let r = renderer();
    let mut bytes = paragraph("one");
    bytes.extend(paragraph("two"));
    let (mut d, closed) = render_b(&bytes, false);
    assert!(d.user_opens_strip(0));
    let open = r.render_window(&d, &WIN);
    let everything = Rect {
        x: 0,
        y: 0,
        w: WIN.cols,
        h: WIN.rows,
    };
    let strip_open = lay(Some((true, 2)));
    let StripPlacement::Placed(s) = strip_open.strip else {
        unreachable!("there is room")
    };
    let total = changed(&r, &closed, &open, &everything);
    assert!(total > 0);
    assert_eq!(
        total,
        changed(&r, &closed, &open, &s),
        "only the strip changed"
    );
    for m in &strip_open.margin {
        assert_eq!(changed(&r, &closed, &open, m), 0);
    }
    assert_eq!(changed(&r, &closed, &open, &strip_open.editor), 0);
}

#[test]
fn foreground_blocks_stay_above_the_editor() {
    let r = renderer();
    let base = baseline();
    let mut many = Vec::new();
    for i in 0..200 {
        many.extend(paragraph(&format!("line {i}")));
    }
    let (_, f) = render_b(&many, true);
    let l = lay(None);
    let content = Rect {
        x: l.content.x,
        y: l.content.y,
        w: l.content.w,
        h: l.editor.y - l.content.y,
    };
    assert!(changed(&r, &base, &f, &content) > 0, "the blocks are drawn");
    let below = Rect {
        x: 0,
        y: l.editor.y,
        w: WIN.cols,
        h: WIN.rows - l.editor.y,
    };
    assert_eq!(
        changed(&r, &base, &f, &below),
        0,
        "nothing at or below the editor"
    );
}
