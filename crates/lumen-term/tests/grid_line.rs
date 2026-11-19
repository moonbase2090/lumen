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

#[test]
fn a_line_is_measured_in_the_terminals_own_clusters() {
    // Plain text: ends on the row it fills, wraps one character later.
    assert_eq!(GridBlock::rows_down(10, 0, "abcdefghij"), 0);
    assert_eq!(GridBlock::rows_down(10, 0, "abcdefghijk"), 1);
    assert_eq!(GridBlock::rows_down(10, 2, "abcdefgh"), 0);
    assert_eq!(GridBlock::rows_down(10, 2, "abcdefghi"), 1);
    // A line break starts the next part at the left edge.
    assert_eq!(GridBlock::rows_down(10, 8, "ab\ncd"), 1);
    // One family emoji is five scalars joined by zero-width joiners: one
    // grapheme two cells wide. It fits in the last two columns; counted a
    // scalar at a time it would not.
    assert_eq!(
        GridBlock::rows_down(10, 8, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
        0
    );
    // A flag is two regional indicators: one grapheme, two cells.
    assert_eq!(GridBlock::rows_down(10, 8, "\u{1F1FA}\u{1F1F8}"), 0);
    // A wide character does not fit in the last column: it starts the next row.
    assert_eq!(GridBlock::rows_down(10, 9, "\u{4E2D}"), 1);
}
