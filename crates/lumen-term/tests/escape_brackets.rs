//! The brackets of a visible escape such as `⟨U+200B⟩` are drawn. The
//! monospace face has no glyph for them, so they come from a bundled fallback
//! face; without it they drew blank, and the escape read as `U+200B` alone.

#![cfg(feature = "zero-width-text")]

use lumen_term::grid::GridBlock;
use lumen_term::notify::{Desk, Notifier, Outcome, Place, Settings, Source, Toast};
use lumen_term::{Document, Frame, Renderer, Window};

const WIN: Window = Window {
    cols: 80,
    rows: 24,
    editor_row: 19,
    editor_rows: 2,
};

fn renderer() -> Renderer {
    Renderer::light(2).unwrap_or_else(|e| unreachable!("the bundled faces load: {e}"))
}

/// The pixels of cells `first..first + cells` of a one-row frame.
fn cells(frame: &Frame, r: &Renderer, first: u32, count: u32) -> Vec<[u8; 3]> {
    let (cw, ch) = r.cell();
    (0..ch)
        .flat_map(|y| (first * cw..(first + count) * cw).map(move |x| (x, y)))
        .map(|(x, y)| frame.pixel(x, y).unwrap_or_default())
        .collect()
}

fn inked(pixels: &[[u8; 3]], paper: [u8; 3]) -> usize {
    pixels.iter().filter(|p| **p != paper).count()
}

fn grid(r: &Renderer, text: &str) -> Frame {
    let (cw, ch) = r.cell();
    let mut block = GridBlock::new(12, 1);
    block.feed(text.as_bytes());
    let mut frame = Frame::new(cw * 12, ch, r.theme().background);
    r.paint_grid(&mut frame, 0, 0, &block);
    frame
}

#[test]
fn a_toast_draws_the_brackets_of_the_escapes_it_shows() {
    let mut n = Notifier::new(Settings {
        allow_unsigned: true,
        ..Settings::default()
    });
    n.set_desk(Desk {
        focused: true,
        muted: false,
    });
    assert!(n.add_layer(1, "tab 1", Place::Local));
    assert_eq!(
        n.notify(0, 1, Source::Sealed, "pay\u{200b}pal \u{202e}evil"),
        Outcome::Toast
    );
    let shown = n.shown()[0].clone();
    // Both the default-ignorable and the bidi code point are escapes.
    assert!(
        shown.text.contains("pay\u{27e8}U+200B\u{27e9}pal")
            && shown.text.contains("\u{27e8}U+202E\u{27e9}evil"),
        "{:?}",
        shown.text
    );
    // The same toast with each bracket turned the other way round. Brackets
    // that draw nothing, or the same missing-glyph box for both, leave the two
    // frames equal; brackets that are drawn tell them apart.
    let flipped = Toast {
        text: shown
            .text
            .chars()
            .map(|c| match c {
                '\u{27e8}' => '\u{27e9}',
                '\u{27e9}' => '\u{27e8}',
                c => c,
            })
            .collect(),
        ..shown.clone()
    };
    let r = renderer();
    let doc = Document::new(76, 4);
    let as_shown = r.render_window_with(&doc, &WIN, &[shown]);
    let turned = r.render_window_with(&doc, &WIN, &[flipped]);
    assert_ne!(as_shown, turned, "the toast's escape brackets are drawn");
}

#[test]
fn the_escape_brackets_and_only_they_draw_from_the_fallback_face() {
    let r = renderer();
    let paper = r.theme().background;
    let frame = grid(&r, "\u{27e8}U+200B\u{27e9}");
    let open = cells(&frame, &r, 0, 1);
    let close = cells(&frame, &r, 7, 1);
    assert!(
        inked(&open, paper) > 20,
        "⟨ drew {} pixels",
        inked(&open, paper)
    );
    assert!(
        inked(&close, paper) > 20,
        "⟩ drew {} pixels",
        inked(&close, paper)
    );
    // A missing-glyph box would be the same shape twice.
    assert_ne!(open, close);
    // Each stays in its own cell: the U next to the opening bracket is not
    // drawn over.
    assert_eq!(cells(&frame, &r, 1, 1), cells(&grid(&r, "U"), &r, 0, 1));
    // The fallback face has a glyph for this bracket too, but it is not one a
    // trust surface puts round an escape, so it draws as it did: nothing.
    assert_eq!(inked(&cells(&grid(&r, "\u{27e6}"), &r, 0, 1), paper), 0);
    // Chinese, Japanese and Korean text still comes from its own face.
    assert!(inked(&cells(&grid(&r, "你"), &r, 0, 2), paper) > 40);
}
