//! Chinese, Japanese and Korean characters the monospace face lacks are drawn
//! from the bundled fallback face, in two cells each.

use lumen_term::{Frame, Renderer, grid::GridBlock};

fn inked(frame: &Frame, x0: u32, x1: u32, y0: u32, y1: u32, paper: [u8; 3]) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.pixel(x, y).is_some_and(|p| p != paper))
        .count()
}

#[test]
fn wide_characters_are_drawn_in_their_two_cells() {
    let renderer = Renderer::light(2).unwrap_or_else(|e| unreachable!("face loads: {e}"));
    let (cw, ch) = renderer.cell();
    let paper = renderer.theme().background;
    let text = "你好あ한";
    let mut block = GridBlock::new(10, 1);
    block.feed(text.as_bytes());
    let mut frame = Frame::new(cw * 10, ch, paper);
    renderer.paint_grid(&mut frame, 0, 0, &block);
    for (index, c) in text.chars().enumerate() {
        let left = u32::try_from(index).unwrap_or(0) * 2 * cw;
        let ink = inked(&frame, left, left + 2 * cw, 0, ch, paper);
        assert!(ink > 40, "{c} drew {ink} pixels in its two cells");
    }
    let past = inked(&frame, 8 * cw, 10 * cw, 0, ch, paper);
    assert_eq!(past, 0, "ink spilled past the last character");
}

#[test]
fn a_wide_character_stays_inside_its_cells() {
    let renderer = Renderer::light(1).unwrap_or_else(|e| unreachable!("face loads: {e}"));
    let (cw, ch) = renderer.cell();
    let paper = renderer.theme().background;
    let mut block = GridBlock::new(6, 1);
    block.feed("a你b".as_bytes());
    let mut frame = Frame::new(cw * 6, ch, paper);
    renderer.paint_grid(&mut frame, 0, 0, &block);
    assert!(inked(&frame, 0, cw, 0, ch, paper) > 0, "a is drawn");
    assert!(inked(&frame, cw, 3 * cw, 0, ch, paper) > 10, "你 is drawn");
    assert!(
        inked(&frame, 3 * cw, 4 * cw, 0, ch, paper) > 0,
        "b is drawn"
    );
}
