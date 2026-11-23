//! A text selection over what the window shows: positions in the lines of the
//! content flow, and the text a selection copies.
//!
//! The renderer lays the document out ([`Renderer::flow`](crate::Renderer::flow))
//! as lines of text, each with where it sits and how it joins the next, so a
//! selection works the same on grid rows and on the wrapped lines of a rich
//! block. A position is a line and a character in it. A copy has the visible
//! lines' text: a line wrapped by the layout joins the next with the blank the
//! wrap dropped (or nothing, where a word was cut), and every other line end is
//! a line feed. Trailing blanks of grid rows are not copied. The text goes
//! through [`copy`](crate::copy::copy), so what a copy strips is stripped.

use crate::copy::{Copied, Kind, Run, copy};

/// How a flow line ends: what a copy puts between it and the next line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// A line feed: a grid row, a code line or the end of a paragraph.
    Break,
    /// The wrap dropped a blank here: a copy keeps it as one space.
    Space,
    /// The wrap cut a word: a copy joins the parts with nothing between.
    Cut,
}

impl Join {
    fn text(self) -> &'static str {
        match self {
            Join::Break => "\n",
            Join::Space => " ",
            Join::Cut => "",
        }
    }
}

/// One line of the content flow that can be selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowLine {
    /// Index of the block in the document.
    pub block: usize,
    /// Pixel row of the line's top, from the top of the content area.
    pub top: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel column of the line's first character, from the content area's left.
    pub left: u32,
    /// The line's text.
    pub text: String,
    /// Whether the line is drawn on the cell grid (grid rows and code) rather
    /// than in the proportional face.
    pub grid: bool,
    /// The step of the type scale a proportional line is set at: 0 for body
    /// text, more for a heading. Always 0 on the grid.
    pub step: i8,
    /// What a copy strips from it.
    pub kind: Kind,
    /// How it joins the next line.
    pub join: Join,
    /// The foreground and background each character was drawn with, so a
    /// selection can redraw it the right way round. Empty when not known.
    pub colours: Vec<([u8; 3], [u8; 3])>,
}

/// A place in the flow: a line, and a character boundary in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    /// Index into the flow's lines.
    pub line: usize,
    /// Characters before the place, from 0 to the line's character count.
    pub col: usize,
}

/// A selection: where it was started and where it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where the press was.
    pub anchor: Pos,
    /// Where the pointer is.
    pub head: Pos,
}

impl Selection {
    /// The two ends, first then last.
    #[must_use]
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The characters of `lines[line]` the selection covers, as a character
    /// range, or `None` if the line is outside it.
    #[must_use]
    pub fn span_in(&self, lines: &[FlowLine], line: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ordered();
        if self.is_empty() || line < a.line || line > b.line {
            return None;
        }
        let len = lines.get(line)?.text.chars().count();
        let from = if line == a.line { a.col.min(len) } else { 0 };
        let to = if line == b.line { b.col.min(len) } else { len };
        (from < to || (line < b.line && line >= a.line)).then_some((from, to.max(from)))
    }
}

/// The whole flow as a selection. It ends at the last line that has text, so
/// the empty rows at the bottom of the window are not copied.
#[must_use]
pub fn select_all(lines: &[FlowLine]) -> Option<Selection> {
    let end = lines
        .iter()
        .rposition(|l| !l.text.trim().is_empty())
        .or(lines.len().checked_sub(1))?;
    Some(Selection {
        anchor: Pos { line: 0, col: 0 },
        head: Pos {
            line: end,
            col: lines[end].text.chars().count(),
        },
    })
}

fn slice(text: &str, from: usize, to: usize) -> String {
    text.chars()
        .skip(from)
        .take(to.saturating_sub(from))
        .collect()
}

/// What copying `sel` from `lines` puts on the clipboard, or `None` for an
/// empty selection.
#[must_use]
pub fn copy_selection(lines: &[FlowLine], sel: &Selection) -> Option<Copied> {
    if sel.is_empty() {
        return None;
    }
    let (a, b) = sel.ordered();
    let mut runs: Vec<Run> = Vec::new();
    for index in a.line..=b.line.min(lines.len().saturating_sub(1)) {
        let line = &lines[index];
        let (from, to) = sel.span_in(lines, index).unwrap_or((0, 0));
        let mut text = slice(&line.text, from, to);
        // The line end is copied only when the selection goes past it.
        let whole_end = to >= line.text.chars().count() && index < b.line;
        if whole_end {
            text.push_str(line.join.text());
        }
        if !text.is_empty() {
            runs.push(Run {
                text,
                kind: line.kind,
                visible: true,
                producer: 0,
            });
        }
    }
    Some(copy(&runs))
}
