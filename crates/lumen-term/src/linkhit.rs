//! Finding links on the grid: the targets a program set with OSC 8, and web
//! addresses written in the text.
//!
//! Nothing here opens anything or judges a target: a [`Hit`] says what is
//! under a cell and who produced it, and [`crate::link::check`] decides what
//! the target may do. A row is read on its own, so an address broken across
//! two rows is found only as far as the first row goes.

use prismattyc_core::{Cell, Screen};
use std::ops::Range;

/// Who made a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Producer {
    /// The program set the target with OSC 8: the text on screen can say anything.
    Program,
    /// The terminal found a web address written in the text.
    Detected,
}

/// A link on one row of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The target, as the program wrote it or as the text reads.
    pub target: String,
    /// The text of the cells the link covers.
    pub visible: String,
    /// Who made it.
    pub producer: Producer,
    /// The columns it covers, from the first to one past the last.
    pub columns: Range<usize>,
}

/// The longest address the scan takes, in characters. A longer run is not a link.
const MAX_DETECTED: usize = 2048;

/// Every link on `row` of `screen`, left to right. A program's OSC 8 link
/// covers its cells; a written address is found only where no OSC 8 link is.
#[must_use]
pub fn row_links(screen: &Screen, row: usize) -> Vec<Hit> {
    let Some(cells) = screen.row(row) else {
        return Vec::new();
    };
    let uri = |col: usize| screen.hyperlink_uri_at_view(0, row, col).map(str::to_owned);
    links_in(cells, &uri)
}

/// Every written address in `cells`, a row of scrollback or of the screen,
/// left to right. The cells alone carry no program link: for a row of
/// scrollback use [`history_links`].
#[must_use]
pub fn cell_links(cells: &[Cell]) -> Vec<Hit> {
    links_in(cells, &|_| None)
}

/// Every link on line `back` of `screen`'s scrollback (0 is the oldest),
/// left to right, the program's as well as written addresses.
#[must_use]
pub fn history_links(screen: &Screen, back: usize) -> Vec<Hit> {
    let hist = screen.max_view_scroll();
    let Some(cells) = screen.scrollback().get(back).filter(|_| back < hist) else {
        return Vec::new();
    };
    // A view scrolled `hist - back` lines shows that line as its first row.
    let offset = hist - back;
    let uri = |col: usize| {
        screen
            .hyperlink_uri_at_view(offset, 0, col)
            .map(str::to_owned)
    };
    links_in(cells, &uri)
}

/// The link under column `col` of `row`, if there is one.
#[must_use]
pub fn link_at(screen: &Screen, row: usize, col: usize) -> Option<Hit> {
    row_links(screen, row)
        .into_iter()
        .find(|hit| hit.columns.contains(&col))
}

fn links_in(cells: &[Cell], uri: &dyn Fn(usize) -> Option<String>) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut col = 0;
    while col < cells.len() {
        let Some(target) = uri(col) else {
            col += 1;
            continue;
        };
        let start = col;
        while col < cells.len() && uri(col).as_deref() == Some(target.as_str()) {
            col += 1;
        }
        hits.push(Hit {
            target,
            visible: text_of(&cells[start..col]),
            producer: Producer::Program,
            columns: start..col,
        });
    }
    let taken: Vec<Range<usize>> = hits.iter().map(|h| h.columns.clone()).collect();
    hits.extend(detect(cells, &taken));
    hits.sort_by_key(|h| h.columns.start);
    hits
}

fn text_of(cells: &[Cell]) -> String {
    cells
        .iter()
        .filter(|c| !c.wide_cont)
        .map(|c| c.character)
        .collect()
}

/// Whether `c` can be part of a written address.
fn in_address(c: char) -> bool {
    !(c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '\0' | '<' | '>' | '"' | '\'' | '`' | '\\' | '{' | '}' | '|' | '^'
        ))
}

/// Web addresses written in `cells`, outside the columns in `taken`.
fn detect(cells: &[Cell], taken: &[Range<usize>]) -> Vec<Hit> {
    // The characters with the column each one starts at; the second half of a
    // wide character is not one.
    let chars: Vec<(usize, char)> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.wide_cont)
        .map(|(i, c)| (i, c.character))
        .collect();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(scheme_len) = scheme_at(&chars, i) else {
            i += 1;
            continue;
        };
        // An address starts at a boundary: not in the middle of a word.
        if i > 0 && chars[i - 1].1.is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut end = i + scheme_len;
        while end < chars.len() && in_address(chars[end].1) {
            end += 1;
        }
        if end - i > MAX_DETECTED {
            // A run this long is not a link at all, not a link to its start.
            i = end;
            continue;
        }
        let end = trim_end(&chars, i + scheme_len, end);
        if end > i + scheme_len {
            let first = chars[i].0;
            let last = chars.get(end).map_or(cells.len(), |c| c.0);
            if !taken.iter().any(|t| first < t.end && t.start < last) {
                let text: String = chars[i..end].iter().map(|c| c.1).collect();
                hits.push(Hit {
                    target: text.clone(),
                    visible: text,
                    producer: Producer::Detected,
                    columns: first..last,
                });
            }
        }
        i = end.max(i + 1);
    }
    hits
}

/// The length of `http://` or `https://` at `i`, if the text starts with it.
fn scheme_at(chars: &[(usize, char)], i: usize) -> Option<usize> {
    for scheme in ["https://", "http://"] {
        let n = scheme.len();
        if chars.len() >= i + n
            && chars[i..i + n]
                .iter()
                .zip(scheme.chars())
                .all(|((_, c), s)| c.eq_ignore_ascii_case(&s))
        {
            return Some(n);
        }
    }
    None
}

/// Drop trailing punctuation that ends a sentence, and a closing bracket the
/// address does not open, from the run `chars[from..end]`.
fn trim_end(chars: &[(usize, char)], from: usize, mut end: usize) -> usize {
    while end > from {
        let c = chars[end - 1].1;
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => unbalanced(chars, from, end, '(', ')'),
            ']' => unbalanced(chars, from, end, '[', ']'),
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

fn unbalanced(chars: &[(usize, char)], from: usize, end: usize, open: char, close: char) -> bool {
    let run = &chars[from..end];
    let opens = run.iter().filter(|c| c.1 == open).count();
    let closes = run.iter().filter(|c| c.1 == close).count();
    closes > opens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridBlock;

    fn row0(bytes: &str) -> Vec<Hit> {
        let mut block = GridBlock::new(60, 3);
        block.feed(bytes.as_bytes());
        row_links(block.screen(), 0)
    }

    fn targets(bytes: &str) -> Vec<(String, Range<usize>)> {
        targets_in(60, bytes)
    }

    fn targets_in(columns: usize, bytes: &str) -> Vec<(String, Range<usize>)> {
        let mut block = GridBlock::new(columns, 3);
        block.feed(bytes.as_bytes());
        row_links(block.screen(), 0)
            .into_iter()
            .map(|h| (h.target, h.columns))
            .collect()
    }

    #[test]
    fn an_address_in_text_is_found_with_its_columns() {
        assert_eq!(
            targets("see https://example.com/a?b=1 now"),
            vec![("https://example.com/a?b=1".to_owned(), 4..29)]
        );
    }

    #[test]
    fn sentence_punctuation_and_a_stray_bracket_are_not_part_of_it() {
        assert_eq!(
            targets("(https://example.com/x).")[0].0,
            "https://example.com/x"
        );
        assert_eq!(
            targets("https://example.com/wiki/A_(b)")[0].0,
            "https://example.com/wiki/A_(b)"
        );
        assert_eq!(targets("go to http://a.test, ok")[0].0, "http://a.test");
    }

    #[test]
    fn a_word_that_only_ends_in_the_scheme_is_not_an_address() {
        assert_eq!(targets("xhttps://example.com").len(), 0);
        assert_eq!(targets("https://").len(), 0);
        assert_eq!(targets("ftp://example.com").len(), 0);
    }

    #[test]
    fn a_run_over_the_limit_is_no_link_and_a_run_at_the_limit_is() {
        let at = format!(
            "https://e.org/{}",
            "a".repeat(MAX_DETECTED - "https://e.org/".len())
        );
        assert_eq!(at.chars().count(), MAX_DETECTED);
        assert_eq!(
            targets_in(2300, &at).len(),
            1,
            "a run of exactly the limit is a link"
        );
        let over = format!("{at}a");
        assert!(
            targets_in(2300, &over).is_empty(),
            "a longer run is not a link"
        );
        let mixed = format!("{over} but https://ok.example/x is");
        assert_eq!(
            targets_in(2300, &mixed)
                .iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>(),
            vec!["https://ok.example/x"]
        );
    }

    #[test]
    fn two_addresses_on_a_row_are_two_links() {
        let all = targets("http://a.test http://b.test");
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].1, 14..27);
    }

    #[test]
    fn a_program_link_covers_its_cells_and_keeps_the_text_it_shows() {
        let hits = row0("go \x1b]8;;https://real.test/\x07docs\x1b]8;;\x07 end");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].producer, Producer::Program);
        assert_eq!(hits[0].target, "https://real.test/");
        assert_eq!(hits[0].visible, "docs");
        assert_eq!(hits[0].columns, 3..7);
    }

    #[test]
    fn an_address_shown_as_the_text_of_a_program_link_is_one_link_not_two() {
        let hits = row0("\x1b]8;;https://other.test/\x07https://shown.test/\x1b]8;;\x07");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].producer, Producer::Program);
        assert_eq!(hits[0].target, "https://other.test/");
    }

    #[test]
    fn a_program_link_that_has_scrolled_into_the_history_is_found_on_its_own_line() {
        let mut block = GridBlock::new(30, 3);
        for n in 0..5 {
            block.feed(
                format!("\x1b]8;;https://t{n}.test/\x07line{n}\x1b]8;;\x07 plain\r\n").as_bytes(),
            );
        }
        let hist = block.history_rows();
        assert!(hist >= 3, "the first lines have scrolled off: {hist}");
        for back in 0..hist {
            let hits = history_links(block.screen(), back);
            assert_eq!(hits.len(), 1, "line {back}: {hits:?}");
            assert_eq!(hits[0].producer, Producer::Program);
            assert_eq!(hits[0].target, format!("https://t{back}.test/"));
            assert_eq!(hits[0].visible, format!("line{back}"));
            assert_eq!(hits[0].columns, 0..5);
        }
        assert_eq!(history_links(block.screen(), hist).len(), 0);
    }
}
