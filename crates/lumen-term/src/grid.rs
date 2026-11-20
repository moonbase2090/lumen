//! A block of terminal cells: an emulator and the text it holds.

use crate::modes::{Changes, ModeTracker};
pub use prismattyc_emulator::CursorShape;
use prismattyc_emulator::Emulator;

/// Scrollback lines kept per block.
pub const SCROLLBACK: usize = 1000;

/// What the program running in a block has asked of its cursor: where it is, whether it is shown
/// (DECTCEM), its shape and whether it blinks (DECSCUSR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorView {
    /// Zero-based row.
    pub row: usize,
    /// Zero-based column. After a write to the last column this can equal the width: the
    /// cursor is then shown on the last cell.
    pub column: usize,
    /// Whether the program wants the cursor shown.
    pub visible: bool,
    /// Block, underline or bar.
    pub shape: CursorShape,
    /// Whether the program asked for a blinking cursor.
    pub blink: bool,
}

/// A block of `columns` by `rows` cells.
///
/// A block holds what an emulator has been fed. It owns no PTY: the bytes of a
/// program reach it only through a [`Session`](crate::Session), which passes
/// them through the arm layer first, so nothing can read a program's output
/// around the layer. The boundary is checked at compile time:
///
/// ```compile_fail
/// let mut block = lumen_term::GridBlock::new(10, 2);
/// let _ = block.poll(); // a block has no way to read a program's output
/// ```
///
/// ```compile_fail
/// use lumen_term::Pty; // the PTY is not part of the public API
/// ```
pub struct GridBlock {
    emulator: Emulator,
    modes: ModeTracker,
    /// Resizes that changed the width. A new width rewraps the history, so a
    /// line number from before it means nothing after it.
    resizes: u64,
    /// Added to the core's count of rows that left the top of the screen, to
    /// number lines. A resize that keeps the width moves rows between the
    /// screen and history without that count changing; this keeps the numbers
    /// steady across it.
    shift: i64,
}

/// A line of a block's primary screen, by number: rows that have left the top
/// of the full screen so far, plus the row on the screen. The number of a line
/// stays the same while output scrolls it up and into history. It is good only
/// while the history has not been rewritten: [`GridBlock::row_of`] gives
/// nothing once the screen was cleared (ED 2, ED 3, RIS) or resized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// The line's number.
    pub number: u64,
    /// The screen's full-clear count when the number was taken.
    clears: u64,
    /// The block's resize count when the number was taken.
    resizes: u64,
}

impl Line {
    /// The line `n` below this one.
    #[must_use]
    pub fn down(self, n: u64) -> Self {
        Self {
            number: self.number.saturating_add(n),
            ..self
        }
    }
}

impl GridBlock {
    /// An empty block, fed by [`feed`](Self::feed).
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            emulator: Emulator::new(columns, rows, SCROLLBACK),
            modes: ModeTracker::new(),
            resizes: 0,
            shift: 0,
        }
    }

    /// The size in columns and rows.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        let screen = self.emulator.screen();
        (screen.columns(), screen.rows())
    }

    /// The screen, for a renderer.
    #[must_use]
    pub fn screen(&self) -> &prismattyc_core::Screen {
        self.emulator.screen()
    }

    /// Give the emulator bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.modes.feed(bytes);
        self.emulator.feed(bytes);
    }

    /// The window title the program set since the last call: `Some("")` is a
    /// reset, `None` is no change.
    pub fn take_title(&mut self) -> Option<String> {
        self.emulator.take_pending_title()
    }

    /// Whether the output rang the bell (BEL as a control, not as the end of
    /// a string) since the last call.
    pub fn take_bell(&mut self) -> bool {
        self.emulator.take_pending_bell()
    }

    /// What the output this block was given did to the key modes.
    #[must_use]
    pub fn key_mode_changes(&self) -> Changes {
        self.modes.changes()
    }

    /// Resize the block.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        let (old_columns, old_rows) = self.size();
        if (old_columns, old_rows) == (columns, rows) {
            self.emulator.resize(columns, rows);
            return;
        }
        if old_columns != columns || self.alt_active() {
            // Rewrapped, or the primary's rows are not in front to follow.
            self.resizes += 1;
            self.emulator.resize(columns, rows);
            return;
        }
        // Only the height changes: the lines keep their wrapping and order,
        // and the cursor stays on its line. Its line keeps its number.
        let before = self.cursor_number();
        self.emulator.resize(columns, rows);
        let after = self.cursor_number();
        self.shift = self.shift.saturating_add(before.saturating_sub(after));
    }

    /// The cursor's line number before [`shift`](Self) is added.
    fn cursor_number(&self) -> i64 {
        let screen = self.emulator.screen();
        let row = i64::try_from(screen.cursor().row).unwrap_or(i64::MAX);
        i64::try_from(screen.scrolled_lines())
            .unwrap_or(i64::MAX)
            .saturating_add(row)
    }

    /// The number of the first history row kept, if numbers can be had.
    fn first_number(&self) -> Option<u64> {
        let screen = self.emulator.screen();
        let history = i64::try_from(self.history_rows()).ok()?;
        let first = i64::try_from(screen.scrolled_lines())
            .ok()?
            .checked_add(self.shift)?
            .checked_sub(history)?;
        u64::try_from(first).ok()
    }

    /// The line `rows_up` rows above the cursor's, on the primary screen.
    /// `None` on the alternate screen, whose rows have no line numbers, or
    /// above the first line.
    #[must_use]
    pub fn line_above_cursor(&self, rows_up: u64) -> Option<Line> {
        let screen = self.emulator.screen();
        if screen.alt_active() {
            return None;
        }
        let number = self
            .cursor_number()
            .checked_add(self.shift)
            .and_then(|n| u64::try_from(n).ok())?;
        Some(Line {
            number: number.checked_sub(rows_up)?,
            clears: screen.full_clears(),
            resizes: self.resizes,
        })
    }

    /// Where `line` is now, counted from the oldest history row kept: history
    /// rows first, then the screen's (the order of [`history_row`] then
    /// [`screen`]). `None` once the line has left the history, the history was
    /// rewritten since it was taken, or the alternate screen is in front.
    ///
    /// [`history_row`]: Self::history_row
    /// [`screen`]: Self::screen
    #[must_use]
    pub fn row_of(&self, line: Line) -> Option<usize> {
        let screen = self.emulator.screen();
        if screen.alt_active()
            || line.clears != screen.full_clears()
            || line.resizes != self.resizes
        {
            return None;
        }
        let history = u64::try_from(self.history_rows()).ok()?;
        let first = self.first_number()?;
        let index = line.number.checked_sub(first)?;
        let rows = u64::try_from(screen.rows()).ok()?;
        (index < history + rows)
            .then(|| usize::try_from(index).ok())
            .flatten()
    }

    /// The column just past the last cell that is not blank in row `index`
    /// ([`row_of`](Self::row_of) order): a wide character's second cell counts
    /// as its own. 0 for a blank row, `None` past the end.
    #[must_use]
    pub fn flow_row_end(&self, index: usize) -> Option<usize> {
        let history = self.history_rows();
        let cells = if index < history {
            self.history_row(index)?
        } else {
            self.emulator.screen().row(index - history)?
        };
        let last = cells
            .iter()
            .rposition(|c| c.wide_cont || c.character != ' ');
        Some(last.map_or(0, |i| i + 1))
    }

    /// The text of row `index` in [`row_of`](Self::row_of) order, trailing
    /// blanks removed.
    #[must_use]
    pub fn flow_row_text(&self, index: usize) -> Option<String> {
        let history = self.history_rows();
        let cells = if index < history {
            self.history_row(index)?
        } else {
            self.emulator.screen().row(index - history)?
        };
        let text: String = cells
            .iter()
            .filter(|c| !c.wide_cont)
            .map(|c| c.character)
            .collect();
        Some(text.trim_end().to_owned())
    }

    /// The text of each row, with trailing blanks removed.
    #[must_use]
    pub fn text_rows(&self) -> Vec<String> {
        let screen = self.emulator.screen();
        (0..screen.rows())
            .map(|r| {
                let line: String = screen
                    .row(r)
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !c.wide_cont)
                    .map(|c| c.character)
                    .collect();
                line.trim_end().to_owned()
            })
            .collect()
    }

    /// The cursor, as the program has set it.
    #[must_use]
    pub fn cursor(&self) -> CursorView {
        let at = self.emulator.screen().cursor();
        CursorView {
            row: at.row,
            column: at.column,
            visible: self.emulator.cursor_visible(),
            shape: self.emulator.cursor_shape(),
            blink: self.emulator.cursor_blink(),
        }
    }

    /// The mouse modes the program running in this block has switched on.
    #[must_use]
    pub fn mouse_modes(&self) -> crate::mouse::Modes {
        crate::mouse::Modes {
            tracking: self.emulator.mouse_tracking(),
            sgr: self.emulator.mouse_sgr(),
            wheel: self.emulator.reports_app_wheel(),
        }
    }

    /// How many lines of history sit above the screen and can be scrolled to.
    /// A program on the alternate screen has none: what it drew is not kept.
    #[must_use]
    pub fn history_rows(&self) -> usize {
        self.emulator.screen().max_view_scroll()
    }

    /// The cells of history line `index`, oldest first, or `None` past the end.
    #[must_use]
    pub fn history_row(&self, index: usize) -> Option<&[prismattyc_core::Cell]> {
        if index >= self.history_rows() {
            return None;
        }
        self.emulator
            .screen()
            .scrollback()
            .get(index)
            .map(Vec::as_slice)
    }

    /// Whether the alternate screen is active.
    #[must_use]
    pub fn alt_active(&self) -> bool {
        self.emulator.screen().alt_active()
    }
}
