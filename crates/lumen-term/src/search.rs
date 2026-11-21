//! Finding text in the document: the rows the window shows or keeps in its
//! scrollback, the matches of what the user typed, and which one is current.
//!
//! The renderer turns the document into rows of text, one for each row of a grid
//! block (its history and its screen) and one for each line of a rich block as it
//! is drawn. This module searches those rows and knows nothing about blocks, so
//! the same rows give the matches, the highlights and the place to scroll to.
//!
//! A row the terminal wrapped on to the next is one line with it, as it is for
//! copying: a match can start at the end of one row and finish on the next, and
//! a real line break still separates lines. The text searched is the text on
//! screen: what the user sees is what they find. The comparison ignores case.

use std::collections::HashMap;

/// One row of text the user can find in, with where it is in the flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRow {
    /// The block the row is in, by place in the document.
    pub block: usize,
    /// The row within the block.
    pub row: usize,
    /// The pixel row of the flow the row starts at.
    pub top: u32,
    /// The row's height in pixels.
    pub height: u32,
    /// The text of the row. A row that wraps keeps its trailing blanks, which
    /// belong to the line.
    pub text: String,
    /// Whether the terminal wrapped this row on to the next row of the block, so
    /// the next row continues this line.
    pub wraps: bool,
}

/// A match: from character `start` of row `row`'s text to character `end` of row
/// `end_row`'s text. A match inside one row has `end_row == row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// The block the match is in.
    pub block: usize,
    /// The row within the block.
    pub row: usize,
    /// The pixel row of the flow the match's row starts at.
    pub top: u32,
    /// The row's height in pixels.
    pub height: u32,
    /// The first character of the match, in row `row`.
    pub start: usize,
    /// The row the match ends in: after `row` when it runs over a wrap.
    pub end_row: usize,
    /// One past the last character of the match, in row `end_row`.
    pub end: usize,
}

/// A span of one row to draw as a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// The first character.
    pub start: usize,
    /// One past the last character.
    pub end: usize,
    /// Whether this is the match the user is on.
    pub current: bool,
}

/// What the window draws for a find: the matches by row, and the bar's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlay {
    /// The matches, by block and row.
    pub marks: HashMap<(usize, usize), Vec<Mark>>,
    /// What the user has typed.
    pub query: String,
    /// The count shown beside it: `3 of 12`, or `No matches`, or empty before
    /// anything is typed.
    pub status: String,
    /// Whether the query has no match.
    pub missing: bool,
}

/// The longest query kept, in characters.
pub const MAX_QUERY: usize = 256;

/// The character `c` compared without case: its lowercase when that is one
/// character, else itself.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(one), None) => one,
        _ => c,
    }
}

/// Every match of `query` in `rows`, in the order of the flow. Matches do not
/// overlap. A row that wraps is searched as one with the row after it. An empty
/// query has none.
#[must_use]
pub fn find_all(rows: &[SearchRow], query: &str) -> Vec<Hit> {
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    let mut first = 0;
    while first < rows.len() {
        let mut end = first + 1;
        while end < rows.len()
            && rows[end - 1].wraps
            && rows[end].block == rows[first].block
            && rows[end].row == rows[end - 1].row + 1
        {
            end += 1;
        }
        find_in_line(&rows[first..end], &needle, &mut hits);
        first = end;
    }
    hits
}

/// The matches of `needle` in the rows of one line (each but the last wraps on
/// to the next), added to `hits`.
fn find_in_line(line: &[SearchRow], needle: &[char], hits: &mut Vec<Hit>) {
    let mut hay: Vec<char> = Vec::new();
    // Where each row's text starts in `hay`.
    let mut starts: Vec<usize> = Vec::with_capacity(line.len());
    for r in line {
        starts.push(hay.len());
        hay.extend(r.text.chars().map(fold));
    }
    let row_of = |at: usize| starts.partition_point(|&s| s <= at) - 1;
    let mut at = 0;
    while at + needle.len() <= hay.len() {
        if hay[at..at + needle.len()] == needle[..] {
            let (from, to) = (row_of(at), row_of(at + needle.len() - 1));
            hits.push(Hit {
                block: line[from].block,
                row: line[from].row,
                top: line[from].top,
                height: line[from].height,
                start: at - starts[from],
                end_row: line[to].row,
                end: at + needle.len() - starts[to],
            });
            at += needle.len();
        } else {
            at += 1;
        }
    }
}

/// An open find: the query, its matches and the current one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Find {
    query: String,
    hits: Vec<Hit>,
    current: Option<usize>,
}

impl Find {
    /// An open find with nothing typed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the user has typed.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The matches, in the order of the flow.
    #[must_use]
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// The current match.
    #[must_use]
    pub fn current(&self) -> Option<&Hit> {
        self.current.and_then(|i| self.hits.get(i))
    }

    /// The place of the current match among the matches, from 0.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current.filter(|i| *i < self.hits.len())
    }

    /// Add `text` to the query, without controls and up to [`MAX_QUERY`]
    /// characters. Returns whether the query changed.
    pub fn push(&mut self, text: &str) -> bool {
        let room = MAX_QUERY.saturating_sub(self.query.chars().count());
        let add: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        if add.is_empty() {
            return false;
        }
        self.query.push_str(&add);
        true
    }

    /// Remove the last character of the query. Returns whether it changed.
    pub fn pop(&mut self) -> bool {
        self.query.pop().is_some()
    }

    /// Search `rows` again for the query. The current match stays on the same
    /// text if it is still a match; otherwise it is the first match at or below
    /// `anchor` (a pixel row of the flow, the top of the view), or the last
    /// match above it when none is below.
    pub fn refresh(&mut self, rows: &[SearchRow], anchor: u32) {
        let kept = self.current().map(|h| (h.block, h.row, h.start));
        self.hits = find_all(rows, &self.query);
        let same = kept.and_then(|(b, r, s)| {
            self.hits
                .iter()
                .position(|h| (h.block, h.row, h.start) == (b, r, s))
        });
        self.current = same.or_else(|| {
            if self.hits.is_empty() {
                None
            } else {
                Some(
                    self.hits
                        .iter()
                        .position(|h| h.top.saturating_add(h.height) > anchor)
                        .unwrap_or(self.hits.len() - 1),
                )
            }
        });
    }

    /// Move to the next match, wrapping from the last to the first. Returns
    /// whether it wrapped, or `None` when there is no match.
    pub fn go_next(&mut self) -> Option<bool> {
        self.step(true)
    }

    /// Move to the previous match, wrapping from the first to the last.
    pub fn go_previous(&mut self) -> Option<bool> {
        self.step(false)
    }

    fn step(&mut self, forward: bool) -> Option<bool> {
        let n = self.hits.len();
        if n == 0 {
            return None;
        }
        let at = self
            .current_index()
            .unwrap_or(if forward { n - 1 } else { 0 });
        let (to, wrapped) = if forward {
            ((at + 1) % n, at + 1 == n)
        } else {
            (if at == 0 { n - 1 } else { at - 1 }, at == 0)
        };
        self.current = Some(to);
        Some(wrapped)
    }

    /// What the window draws for this find.
    #[must_use]
    pub fn overlay(&self) -> Overlay {
        let mut marks: HashMap<(usize, usize), Vec<Mark>> = HashMap::new();
        let current = self.current_index();
        for (i, h) in self.hits.iter().enumerate() {
            let is_current = Some(i) == current;
            for row in h.row..=h.end_row {
                // A match over a wrap marks the rest of its first row, all of
                // the rows between, and the start of its last.
                let start = if row == h.row { h.start } else { 0 };
                let end = if row == h.end_row { h.end } else { usize::MAX };
                marks.entry((h.block, row)).or_default().push(Mark {
                    start,
                    end,
                    current: is_current,
                });
            }
        }
        let missing = !self.query.is_empty() && self.hits.is_empty();
        let status = if self.query.is_empty() {
            String::new()
        } else if missing {
            "No matches".to_owned()
        } else {
            format!("{} of {}", current.map_or(0, |i| i + 1), self.hits.len())
        };
        Overlay {
            marks,
            query: self.query.clone(),
            status,
            missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<SearchRow> {
        lines
            .iter()
            .enumerate()
            .map(|(i, t)| SearchRow {
                block: 0,
                row: i,
                top: u32::try_from(i).unwrap_or(0) * 10,
                height: 10,
                text: (*t).to_owned(),
                wraps: false,
            })
            .collect()
    }

    /// Rows as `rows`, with the ones in `wrapping` wrapped on to the next.
    fn wrapped(lines: &[&str], wrapping: &[usize]) -> Vec<SearchRow> {
        let mut r = rows(lines);
        for &i in wrapping {
            r[i].wraps = true;
        }
        r
    }

    #[test]
    fn a_hit_a_miss_and_case() {
        let r = rows(&["alpha Beta", "gamma", "BETA beta"]);
        let hits = find_all(&r, "beta");
        let spots: Vec<(usize, usize)> = hits.iter().map(|h| (h.row, h.start)).collect();
        assert_eq!(spots, [(0, 6), (2, 0), (2, 5)]);
        assert_eq!(find_all(&r, "delta").len(), 0);
        assert_eq!(find_all(&r, "").len(), 0);
    }

    #[test]
    fn matches_do_not_overlap_or_span_a_real_line_break() {
        let r = rows(&["aaaa", "ab", "ba"]);
        assert_eq!(find_all(&r, "aa").len(), 2);
        assert_eq!(
            find_all(&r, "abba").len(),
            0,
            "a line break separates lines"
        );
    }

    #[test]
    fn a_match_runs_over_a_wrap() {
        let r = wrapped(&["xxxnee", "dle", "needle"], &[0]);
        let hits = find_all(&r, "NEEDLE");
        let spans: Vec<(usize, usize, usize, usize)> = hits
            .iter()
            .map(|h| (h.row, h.start, h.end_row, h.end))
            .collect();
        // Over the wrap, then the whole of the third row, which is its own line.
        assert_eq!(spans, [(0, 3, 1, 3), (2, 0, 2, 6)]);
    }

    #[test]
    fn a_wrap_joins_only_the_rows_it_joins() {
        // Row 1 does not wrap, so row 2 is a new line.
        let r = wrapped(&["nee", "dle", "needle"], &[0]);
        assert_eq!(find_all(&r, "needle").len(), 2);
        let unjoined = rows(&["nee", "dle"]);
        assert_eq!(find_all(&unjoined, "needle").len(), 0);
    }

    #[test]
    fn a_match_can_run_over_more_than_two_rows() {
        let r = wrapped(&["ab", "cd", "ef"], &[0, 1]);
        let hits = find_all(&r, "bcde");
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].row, hits[0].start), (0, 1));
        assert_eq!((hits[0].end_row, hits[0].end), (2, 1));
    }

    #[test]
    fn a_match_over_a_wrap_is_marked_on_both_rows() {
        let r = wrapped(&["xxxnee", "dle"], &[0]);
        let mut f = Find::new();
        f.push("needle");
        f.refresh(&r, 0);
        let o = f.overlay();
        let first = &o.marks[&(0, 0)];
        let second = &o.marks[&(0, 1)];
        assert_eq!(
            (first[0].start, first[0].end > 6, first[0].current),
            (3, true, true)
        );
        assert_eq!(
            (second[0].start, second[0].end, second[0].current),
            (0, 3, true)
        );
        assert_eq!(o.status, "1 of 1");
    }

    #[test]
    fn next_and_previous_wrap() {
        let r = rows(&["x", "x", "x"]);
        let mut f = Find::new();
        f.push("x");
        f.refresh(&r, 0);
        assert_eq!(f.current_index(), Some(0));
        assert_eq!(f.go_next(), Some(false));
        assert_eq!(f.go_next(), Some(false));
        assert_eq!(f.current_index(), Some(2));
        assert_eq!(f.go_next(), Some(true));
        assert_eq!(f.current_index(), Some(0));
        assert_eq!(f.go_previous(), Some(true));
        assert_eq!(f.current_index(), Some(2));
    }

    #[test]
    fn the_first_match_is_at_or_below_the_anchor_else_the_last_above() {
        let r = rows(&["x", "y", "x", "y", "x"]);
        let mut f = Find::new();
        f.push("x");
        f.refresh(&r, 15);
        assert_eq!(f.current().map(|h| h.row), Some(2));
        // A search with nothing kept and the anchor below every match.
        let mut g = Find::new();
        g.push("x");
        g.refresh(&r, 500);
        assert_eq!(g.current().map(|h| h.row), Some(4));
    }

    #[test]
    fn the_current_match_stays_on_its_text_as_output_is_added_below() {
        let mut f = Find::new();
        f.push("x");
        f.refresh(&rows(&["x", "y", "x"]), 0);
        f.go_next();
        assert_eq!(f.current().map(|h| h.row), Some(2));
        f.refresh(&rows(&["x", "y", "x", "y", "x"]), 0);
        assert_eq!(f.current().map(|h| h.row), Some(2));
        assert_eq!(f.hits().len(), 3);
    }

    #[test]
    fn the_overlay_says_which_is_current_and_how_many() {
        let r = rows(&["x x", "x"]);
        let mut f = Find::new();
        f.push("x");
        f.refresh(&r, 0);
        f.go_next();
        let o = f.overlay();
        assert_eq!(o.status, "2 of 3");
        assert_eq!(o.marks[&(0, 0)].iter().filter(|m| m.current).count(), 1);
        assert!(!o.missing);
        f.push("zz");
        f.refresh(&r, 0);
        let o = f.overlay();
        assert_eq!(o.status, "No matches");
        assert!(o.missing && o.marks.is_empty());
    }

    #[test]
    fn the_query_has_no_controls_and_a_cap() {
        let mut f = Find::new();
        f.push("a\u{1b}[31mb\n");
        assert_eq!(f.query(), "a[31mb");
        f.push(&"z".repeat(1000));
        assert_eq!(f.query().chars().count(), MAX_QUERY);
        assert!(f.pop());
    }
}
