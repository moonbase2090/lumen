//! A line of a grid block keeps its number while output scrolls it up and into
//! history, and the number is dropped once the history is rewritten: a clear,
//! a reset or a resize. A real emulator, fed real bytes.

use lumen_term::GridBlock;

fn lines(block: &mut GridBlock, from: usize, to: usize) {
    for n in from..to {
        block.feed(format!("line {n}\r\n").as_bytes());
    }
}

/// The text of the row `line` is on now, if it still has one.
fn text_of(block: &GridBlock, line: lumen_term::Line) -> Option<String> {
    block.row_of(line).and_then(|i| block.flow_row_text(i))
}

#[test]
fn a_line_keeps_its_text_as_it_scrolls_into_history() {
    let mut block = GridBlock::new(20, 5);
    block.feed(b"first\r\n");
    lines(&mut block, 0, 2);
    block.feed(b"anchored\r\n");
    let line = block
        .line_above_cursor(1)
        .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
    assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
    // Well past the screen's height: the line is in history now.
    lines(&mut block, 2, 40);
    assert!(block.row_of(line).is_some_and(|i| i < block.history_rows()));
    assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
    assert_eq!(
        block
            .line_above_cursor(1)
            .and_then(|l| text_of(&block, l))
            .as_deref(),
        Some("line 39"),
        "the newest line has its own number"
    );
}

#[test]
fn a_scroll_inside_a_margin_region_does_not_move_line_numbers() {
    let mut block = GridBlock::new(20, 6);
    block.feed(b"top\r\nanchored\r\n");
    let line = block
        .line_above_cursor(1)
        .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
    // Rows 4 to 6 scroll among themselves; nothing leaves the top of the
    // screen, so nothing above them moves.
    block.feed(b"\x1b[4;6r\x1b[6;1H");
    for n in 0..10 {
        block.feed(format!("region {n}\n\r").as_bytes());
    }
    block.feed(b"\x1b[r");
    assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
}

#[test]
fn a_clear_a_reset_or_a_resize_drops_the_line() {
    for rewrite in [&b"\x1b[2J"[..], b"\x1b[3J", b"\x1bc"] {
        let mut block = GridBlock::new(20, 5);
        block.feed(b"anchored\r\n");
        let line = block
            .line_above_cursor(1)
            .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
        assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
        block.feed(rewrite);
        assert_eq!(block.row_of(line), None, "{rewrite:?}");
    }
    let mut block = GridBlock::new(20, 5);
    block.feed(b"anchored\r\n");
    let line = block
        .line_above_cursor(1)
        .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
    block.resize(20, 5);
    assert_eq!(
        text_of(&block, line).as_deref(),
        Some("anchored"),
        "the same size is no resize"
    );
    block.resize(12, 5);
    assert_eq!(block.row_of(line), None, "the history was rewrapped");
}

#[test]
fn a_change_of_height_alone_keeps_every_line_where_it_is() {
    // (rows before, rows after, lines written before the anchor and after it)
    for (before, after, count) in [(10, 6, 3), (10, 6, 12), (6, 10, 12), (10, 4, 8)] {
        let mut block = GridBlock::new(20, before);
        lines_and_anchor(&mut block, count);
        let line = block
            .line_above_cursor(1)
            .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
        let earlier = block
            .line_above_cursor(3)
            .unwrap_or_else(|| unreachable!("there are lines above"));
        assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
        let earlier_text = text_of(&block, earlier);
        block.resize(20, after);
        let case = format!("{before} to {after} rows, {count} lines");
        assert_eq!(text_of(&block, line).as_deref(), Some("anchored"), "{case}");
        assert_eq!(text_of(&block, earlier), earlier_text, "{case}");
        // Numbers given after the resize agree with the ones from before it.
        assert_eq!(block.line_above_cursor(1), Some(line), "{case}");
        // And still follow output as it scrolls on.
        lines(&mut block, 0, 30);
        assert_eq!(text_of(&block, line).as_deref(), Some("anchored"), "{case}");
    }
}

/// `count` numbered lines, then one that reads "anchored".
fn lines_and_anchor(block: &mut GridBlock, count: usize) {
    lines(block, 0, count);
    block.feed(b"anchored\r\n");
}

#[test]
fn the_alternate_screen_has_no_line_numbers_and_the_primary_keeps_its_own() {
    let mut block = GridBlock::new(20, 5);
    block.feed(b"anchored\r\n");
    let line = block
        .line_above_cursor(1)
        .unwrap_or_else(|| unreachable!("the primary screen has line numbers"));
    block.feed(b"\x1b[?1049h");
    assert_eq!(block.line_above_cursor(0), None);
    assert_eq!(block.row_of(line), None, "the primary is not in front");
    for n in 0..20 {
        block.feed(format!("tui {n}\r\n").as_bytes());
    }
    block.feed(b"\x1b[?1049l");
    assert_eq!(text_of(&block, line).as_deref(), Some("anchored"));
}
