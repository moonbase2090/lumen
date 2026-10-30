//! Zero-width, joining and bidirectional characters in paragraph text, and the
//! escapes that trust surfaces use instead.

#![cfg(feature = "zero-width-text")]
#![allow(clippy::many_single_char_names)]

use lumen_detector::TAG;
use lumen_term::{Block, Context, Document, Renderer, Window, wrap_text};

const WIN: Window = Window {
    cols: 60,
    rows: 12,
    editor_row: 8,
    editor_rows: 2,
};

fn renderer() -> Renderer {
    Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

fn paragraph_frame(text: &str) -> Vec<u8> {
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

fn document(texts: &[&str]) -> Document {
    let mut d = Document::new(56, 6);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    lumen_term::Layer::open_container(&mut layer);
    for t in texts {
        d.receive(&mut layer, &paragraph_frame(t), Context::default());
    }
    d
}

fn rich_texts(d: &Document) -> Vec<String> {
    d.blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Rich(r) => match &r.content {
                lumen_track0::Block::Paragraph(t) => Some(t.clone()),
                _ => None,
            },
            Block::Grid(_) => None,
        })
        .collect()
}

#[test]
fn characters_that_draw_nothing_change_no_pixel_and_no_width() {
    let r = renderer();
    let plain = r.render_window(&document(&["ab cd"]), &WIN);
    for invisible in [
        "a\u{200b}b cd",
        "a\u{2060}b cd",
        "a\u{200d}b cd",
        "a\u{200c}b cd",
        "a\u{fe0f}b cd",
        "a\u{feff}b cd",
        "a\u{202e}b cd",
        "a\u{2067}b cd",
        "a\u{200f}b cd",
        "ab\u{200b} cd",
    ] {
        assert_eq!(
            r.render_window(&document(&[invisible]), &WIN),
            plain,
            "{invisible:?} must not be drawn"
        );
    }
}

#[test]
fn the_text_the_block_holds_keeps_every_code_point_for_copy_and_select() {
    let d = document(&["a\u{200d}b\u{200b}c\u{200c}d\u{fe0f}"]);
    assert_eq!(rich_texts(&d), ["a\u{200d}b\u{200b}c\u{200c}d\u{fe0f}"]);
}

#[test]
fn a_right_to_left_override_in_one_block_cannot_reach_the_next() {
    let d = document(&["first \u{202e}reversed", "second", "third \u{2067}isolated"]);
    let t = rich_texts(&d);
    assert_eq!(t.len(), 3);
    assert_eq!(
        t[0], "first \u{202e}reversed\u{202c}",
        "closed at the end of its block"
    );
    assert_eq!(t[1], "second", "the next block carries no control at all");
    assert_eq!(t[2], "third \u{2067}isolated\u{2069}");
}

#[test]
fn a_zero_width_space_is_a_place_to_break_a_line_and_is_never_drawn() {
    // Without a space a long word breaks anywhere; with the zero width space
    // the line breaks there.
    assert_eq!(wrap_text("ab\u{200b}cdef", 4), ["ab\u{200b}", "cdef"]);
    // It takes no cells: "abcd" and "ab<ZWSP>cd" fit the same line.
    assert_eq!(wrap_text("ab\u{200b}cd", 4), ["ab\u{200b}cd"]);
    // Every character is kept for copy.
    assert_eq!(wrap_text("ab\u{200b}cdef", 4).concat(), "ab\u{200b}cdef");
}

#[test]
fn a_word_joiner_and_a_joiner_are_never_the_place_a_line_breaks() {
    // Width 2 would cut "ab" from the rest; the word joiner forbids a cut beside it.
    let lines = wrap_text("ab\u{2060}cd", 2);
    for pair in lines.windows(2) {
        let end = pair[0].chars().last();
        let start = pair[1].chars().next();
        assert_ne!(end, Some('\u{2060}'), "{lines:?}");
        assert_ne!(start, Some('\u{2060}'), "{lines:?}");
    }
    assert_eq!(lines.concat(), "ab\u{2060}cd");
    // An emoji sequence is whole on a line wide enough to hold it.
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    for width in 6..=14 {
        let lines = wrap_text(&format!("aa {family} bb {family}"), width);
        assert!(
            lines
                .iter()
                .filter(|l| l.contains('\u{1f468}'))
                .all(|l| l.contains(family)),
            "width {width}: {lines:?}"
        );
        for l in &lines {
            assert!(
                !l.starts_with('\u{200d}') && !l.ends_with('\u{200d}'),
                "width {width}: {lines:?}"
            );
        }
    }
    // When the line cannot hold the sequence at all, it is cut rather than lost.
    assert_eq!(wrap_text(family, 4).concat(), family);
}

/// The cell width of each character in the property test's alphabet, written
/// out here so the expectation does not depend on the wrapping code.
fn known_width(c: char) -> usize {
    match c {
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{fe0f}' | '\u{202e}' => 0,
        '\u{1f468}' | '\u{4e2d}' => 2,
        _ => 1,
    }
}

#[test]
fn wrapping_loses_no_character_whatever_the_text_and_width() {
    let alphabet: Vec<char> =
        "ab \u{200b}\u{200d}\u{200c}\u{2060}\u{fe0f}\u{1f468}\u{4e2d}\u{645}\u{202e}"
            .chars()
            .collect();
    let mut state = 99_u64;
    for width in 1..=9 {
        for _ in 0..400 {
            let mut text = String::new();
            for _ in 0..30 {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                text.push(alphabet[(state >> 33) as usize % alphabet.len()]);
            }
            let lines = wrap_text(&text, width);
            let kept: String = lines.concat().chars().filter(|c| *c != ' ').collect();
            let want: String = text.chars().filter(|c| *c != ' ').collect();
            // A character wider than the line cannot be shown and is dropped
            // whole; everything else must be there, in order. The widths come
            // from this test's own table, not from the code under test.
            let shown = |c: char| known_width(c) <= width;
            let want: String = want.chars().filter(|c| shown(*c)).collect();
            let kept: String = kept.chars().filter(|c| shown(*c)).collect();
            assert_eq!(kept, want, "width {width}: {text:?}");
        }
    }
}

#[test]
fn a_toast_shows_zero_width_and_bidi_characters_as_escapes() {
    use lumen_term::notify::{Desk, Notifier, Outcome, Place, Settings, Source};
    let mut n = Notifier::new(Settings {
        os_notifications: true,
        os_text: true,
        ..Settings::default()
    });
    n.set_desk(Desk {
        focused: true,
        muted: false,
    });
    n.add_layer(1, "tab 1", Place::Local);
    assert_eq!(
        n.notify(0, 1, Source::Sealed, "pay\u{200b}pal \u{202e}evil"),
        Outcome::Toast
    );
    let toast = &n.shown()[0];
    assert!(
        toast.text.contains("pay\u{27e8}U+200B\u{27e9}pal"),
        "{}",
        toast.text
    );
    assert!(
        toast.text.contains("\u{27e8}U+202E\u{27e9}evil"),
        "{}",
        toast.text
    );
    assert!(!toast.text.contains('\u{200b}') && !toast.text.contains('\u{202e}'));
}
