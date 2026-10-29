//! The headless rasteriser: determinism, colours, the container and layout.
#![allow(
    clippy::many_single_char_names,
    reason = "short names for the renderer, document, frame and theme keep the pixel checks readable"
)]

use lumen_term::raster::CONTAINER_LABEL;
use lumen_term::{Block, Context, Document, Frame, Passthrough, Piece, Renderer};
use lumen_track0::Block as Track0Block;
use sha2::{Digest, Sha256};

fn renderer(scale: u32) -> Renderer {
    Renderer::light(scale).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

fn doc(text: &[u8], columns: usize, rows: usize) -> Document {
    let mut d = Document::new(columns, rows);
    d.receive(&mut Passthrough, text, Context::default());
    d
}

fn digest(f: &Frame) -> String {
    format!("{:x}", Sha256::digest(&f.rgba))
}

const WHITE: [u8; 3] = [255, 255, 255];
const INK: [u8; 3] = [28, 28, 28];

fn has_pixel(f: &Frame, x0: u32, y0: u32, w: u32, h: u32, want: [u8; 3]) -> bool {
    (y0..y0 + h).any(|y| (x0..x0 + w).any(|x| f.pixel(x, y) == Some(want)))
}

#[test]
fn the_cell_size_is_fixed_by_the_face_and_the_scale() {
    assert_eq!(renderer(1).cell(), (8, 19));
    assert_eq!(renderer(2).cell(), (17, 37));
}

#[test]
fn the_same_document_renders_to_the_same_pixels_every_time() {
    let r = renderer(2);
    let d = doc(b"Hello, World\r\n\x1b[31mred\x1b[0m", 20, 3);
    let (a, b) = (r.render(&d), r.render(&d));
    assert_eq!(a, b, "the baseline taken twice must match exactly");
    assert_eq!((a.width, a.height), (340, 111));
    assert_eq!(
        digest(&a),
        "9bf629567863298ae5fa097a36a18f0b6f00282fb4e1dfdeceba6d944d8d17e4"
    );
}

#[test]
fn another_renderer_of_the_same_scale_gives_the_same_pixels() {
    let d = doc(b"abc", 10, 2);
    assert_eq!(renderer(2).render(&d), renderer(2).render(&d));
}

#[test]
fn different_text_gives_different_pixels() {
    let r = renderer(2);
    let (a, b) = (r.render(&doc(b"a", 10, 2)), r.render(&doc(b"b", 10, 2)));
    assert_ne!(digest(&a), digest(&b));
}

#[test]
fn a_glyph_stays_in_its_cell_and_the_blank_is_the_background() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let f = r.render(&doc(b"A", 10, 2));
    assert!(
        (0..ch).any(|y| (0..cw).any(|x| f.pixel(x, y) != Some(WHITE))),
        "the glyph is painted in cell 0"
    );
    assert!(
        !has_pixel(&f, cw, 0, 9 * cw, ch, INK),
        "the next cells are blank"
    );
    assert_eq!(f.pixel(f.width - 1, f.height - 1), Some(WHITE));
}

#[test]
fn colours_come_from_the_theme_and_from_truecolor() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let red = r.theme().ansi[1];
    let f = r.render(&doc(b"\x1b[31mH\x1b[0m\x1b[38;2;10;20;30mH", 4, 1));
    assert!(has_pixel(&f, 0, 0, cw, ch, red), "ANSI red text");
    assert!(
        has_pixel(&f, cw, 0, cw, ch, [10, 20, 30]),
        "true-colour text"
    );
}

#[test]
fn a_glyph_the_face_lacks_is_skipped_and_its_cells_keep_their_background() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let f = r.render(&doc("\u{6f22}".as_bytes(), 4, 1));
    assert!(
        (0..2 * cw).all(|x| (0..ch).all(|y| f.pixel(x, y) == Some(WHITE))),
        "no ink for a character the face does not have"
    );
}

#[test]
fn an_indexed_colour_uses_the_cube_and_the_grey_ramp() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let f = r.render(&doc(b"\x1b[38;5;196mH\x1b[38;5;240mH\x1b[38;5;67mH", 4, 1));
    assert!(has_pixel(&f, 0, 0, cw, ch, [255, 0, 0]));
    assert!(has_pixel(&f, cw, 0, cw, ch, [88, 88, 88]));
    assert!(has_pixel(&f, 2 * cw, 0, cw, ch, [95, 135, 175]));
}

#[test]
fn inverse_swaps_foreground_and_background() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let f = r.render(&doc(b"\x1b[7m \x1b[0m ", 4, 1));
    assert_eq!(
        f.pixel(1, 1),
        Some(INK),
        "the inverse cell is filled with the foreground"
    );
    assert_eq!(f.pixel(cw + 1, 1), Some(WHITE));
    assert_eq!(f.pixel(cw - 1, ch - 1), Some(INK));
}

#[test]
fn underline_paints_the_bottom_rows_of_the_cell() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let plain = r.render(&doc(b"x", 4, 1));
    let under = r.render(&doc(b"\x1b[4mx", 4, 1));
    assert_eq!(under.pixel(0, ch - 1), Some(INK));
    assert_eq!(under.pixel(cw - 1, ch - 2), Some(INK));
    assert_ne!(plain.pixel(0, ch - 1), Some(INK));
}

#[test]
fn a_rich_block_is_drawn_in_a_labelled_container() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let mut d = Document::new(30, 2);
    d.apply(vec![Piece::Rich(Track0Block::Paragraph("hello".into()))]);
    let f = r.render(&d);
    let t = r.theme();
    assert_eq!(f.pixel(0, 0), Some(t.border));
    assert_eq!(f.pixel(2, 2), Some(t.container));
    assert!(
        has_pixel(&f, cw, 2, 40 * cw / 2, ch, t.label),
        "the label {CONTAINER_LABEL:?} is painted in the label colour"
    );
    assert!(has_pixel(&f, cw, 2 + ch, 10 * cw, ch, INK), "the body text");
    let Block::Rich(b) = &d.blocks()[0] else {
        unreachable!("a rich block")
    };
    assert_eq!(f.height, r.rich_height(b, 30));
    assert_eq!(
        f.height,
        2 * 2 + 2 * ch,
        "border, label row and one body row"
    );
}

#[test]
fn headings_code_and_the_stripped_count_are_drawn() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let t = r.theme().clone();
    let mut d = Document::new(30, 2);
    d.apply(vec![
        Piece::Rich(Track0Block::Heading {
            level: 1,
            text: "Title".into(),
        }),
        Piece::Rich(Track0Block::Code {
            lang: String::new(),
            text: "a\nb".into(),
            stripped: 2,
        }),
    ]);
    let f = r.render(&d);
    assert!(
        has_pixel(&f, cw, 2 + ch, 6 * cw, ch, t.heading),
        "heading colour"
    );
    let Block::Rich(code) = &d.blocks()[1] else {
        unreachable!("rich")
    };
    let y0 = r.rich_height(
        match &d.blocks()[0] {
            Block::Rich(h) => h,
            Block::Grid(_) => unreachable!("rich"),
        },
        30,
    );
    assert_eq!(f.pixel(3, y0 + 2 + ch + 1), Some(t.code), "code background");
    assert_eq!(
        f.height,
        y0 + r.rich_height(code, 30),
        "two code lines plus the stripped note"
    );
    assert_eq!(r.rich_height(code, 30), 2 * 2 + (1 + 3) * ch);
}

#[test]
fn a_long_paragraph_wraps_and_a_long_word_is_broken() {
    let r = renderer(1);
    let (_, ch) = r.cell();
    let one = |text: &str| {
        let mut d = Document::new(12, 1);
        d.apply(vec![Piece::Rich(Track0Block::Paragraph(text.into()))]);
        let Block::Rich(b) = &d.blocks()[0] else {
            unreachable!("rich")
        };
        (r.rich_height(b, 12) - 2) / ch - 1
    };
    assert_eq!(one("short"), 1);
    assert_eq!(one("aaaa bbbbb"), 1, "exactly ten cells fits");
    assert_eq!(one("aaaa bbbbbb"), 2, "eleven cells does not");
    assert_eq!(one("aaa bbb ccc ddd eee fff"), 3, "wraps at ten cells");
    assert_eq!(
        one("abcdefghijklmnopqrstuvwxyz"),
        3,
        "a long word breaks at ten cells"
    );
    assert_eq!(one("a\nb"), 2, "a newline is a line break");
}

#[test]
fn a_document_stacks_its_blocks_top_to_bottom() {
    let r = renderer(1);
    let (cw, ch) = r.cell();
    let mut d = doc(b"top", 20, 2);
    d.apply(vec![Piece::Rich(Track0Block::Paragraph("mid".into()))]);
    d.receive(&mut Passthrough, b"bottom", Context::default());
    let f = r.render(&d);
    let rich = 2 + 2 * ch;
    assert_eq!(f.width, 20 * cw);
    assert_eq!(f.height, 2 * ch + rich + 2 * ch);
    assert!(has_pixel(&f, 0, 0, 3 * cw, ch, INK), "top text");
    assert_eq!(
        f.pixel(0, 2 * ch),
        Some(r.theme().border),
        "the container starts after the grid block"
    );
    assert!(
        has_pixel(&f, 0, 2 * ch + rich, 6 * cw, ch, INK),
        "bottom text"
    );
}

#[test]
fn a_frame_round_trips_through_png() {
    let r = renderer(1);
    let f = r.render(&doc(b"png", 8, 1));
    let Ok(bytes) = f.to_png() else {
        unreachable!("png encodes")
    };
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap_or_else(|e| unreachable!("png reads: {e}"));
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .unwrap_or_else(|e| unreachable!("png frame: {e}"));
    assert_eq!((info.width, info.height), (f.width, f.height));
    assert_eq!(&buf[..info.buffer_size()], &f.rgba[..]);
}

#[test]
fn a_wide_character_fills_both_of_its_cells() {
    let r = renderer(2);
    let (cw, ch) = r.cell();
    let red = r.theme().ansi[1];
    let f = r.render(&doc("\x1b[41m\u{6f22}\x1b[0m.".as_bytes(), 6, 1));
    assert_eq!(f.pixel(1, 1), Some(red), "the first cell");
    assert_eq!(
        f.pixel(cw + 1, 1),
        Some(red),
        "the second cell of the wide character"
    );
    assert_eq!(
        f.pixel(2 * cw + 1, ch - 1),
        Some(WHITE),
        "the next cell is not filled"
    );
}
