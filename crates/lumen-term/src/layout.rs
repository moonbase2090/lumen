//! Where the terminal's own parts sit: the trust margin, the input editor and
//! the background strip.
//!
//! This is geometry in cells, with no pixels. It exists so the rules can be
//! tested before a window draws them, and so the 2.3 pixel diff has the
//! rectangles it needs: the trust margin is the gutter and the header strip,
//! the terminal draws every pixel of it, and nothing a producer writes can move
//! or reach it.

/// A rectangle of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left column.
    pub x: u32,
    /// Top row.
    pub y: u32,
    /// Width in columns.
    pub w: u32,
    /// Height in rows.
    pub h: u32,
}

impl Rect {
    /// The row below the rectangle.
    #[must_use]
    pub fn bottom(&self) -> u32 {
        self.y.saturating_add(self.h)
    }

    /// Whether the two rectangles share a cell.
    #[must_use]
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.x.saturating_add(other.w)
            && other.x < self.x.saturating_add(self.w)
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Whether `other` lies inside this rectangle.
    #[must_use]
    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x.saturating_add(other.w) <= self.x.saturating_add(self.w)
            && other.bottom() <= self.bottom()
    }
}

/// Columns of the gutter, on the left, the full height of the window.
pub const GUTTER_COLUMNS: u32 = 4;
/// Rows of the header strip, along the top, the full width of the window.
pub const HEADER_ROWS: u32 = 1;
/// Rows of a collapsed strip.
pub const STRIP_COLLAPSED_ROWS: u32 = 1;

/// How the strip is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripPlacement {
    /// There is no strip in this cycle.
    None,
    /// The strip sits below the editor.
    Placed(Rect),
    /// There is a strip but no row below the editor for it. The editor does
    /// not move to make room; the terminal chose its row when it placed it.
    NoRoom,
}

/// The parts of the window, for one layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The trust margin: the gutter and the header strip. Terminal-drawn only.
    pub margin: [Rect; 2],
    /// Everything that is not the margin.
    pub content: Rect,
    /// The input editor.
    pub editor: Rect,
    /// The background strip.
    pub strip: StripPlacement,
}

/// Lay out a window of `cols` by `rows`, with the editor of `editor_rows` rows
/// starting at row `editor_row`.
///
/// `strip` is `None` for no strip, otherwise `Some((open, blocks))`. The
/// editor's rectangle and the margin depend only on the window and the
/// editor's own row, never on the strip, so a strip appearing, growing or
/// opening cannot shift the prompt or paint the margin. An open strip takes up
/// to one row per block below its header, as far down as the window goes.
#[must_use]
pub fn layout(
    cols: u32,
    rows: u32,
    editor_row: u32,
    editor_rows: u32,
    strip: Option<(bool, usize)>,
) -> Layout {
    let gutter = Rect {
        x: 0,
        y: 0,
        w: GUTTER_COLUMNS.min(cols),
        h: rows,
    };
    let header = Rect {
        x: 0,
        y: 0,
        w: cols,
        h: HEADER_ROWS.min(rows),
    };
    let content = Rect {
        x: gutter.w,
        y: header.h,
        w: cols.saturating_sub(gutter.w),
        h: rows.saturating_sub(header.h),
    };
    let editor = Rect {
        x: content.x,
        y: editor_row.max(content.y).min(rows),
        w: content.w,
        h: editor_rows.min(rows.saturating_sub(editor_row.max(content.y))),
    };
    let below = rows.saturating_sub(editor.bottom());
    let strip = match strip {
        None => StripPlacement::None,
        Some(_) if below < STRIP_COLLAPSED_ROWS => StripPlacement::NoRoom,
        Some((open, blocks)) => {
            let wanted = if open {
                STRIP_COLLAPSED_ROWS.saturating_add(u32::try_from(blocks).unwrap_or(u32::MAX))
            } else {
                STRIP_COLLAPSED_ROWS
            };
            StripPlacement::Placed(Rect {
                x: content.x,
                y: editor.bottom(),
                w: content.w,
                h: wanted.min(below),
            })
        }
    };
    Layout {
        margin: [gutter, header],
        content,
        editor,
        strip,
    }
}

/// Columns of the notification area, at most.
pub const TOAST_COLUMNS: u32 = 36;

/// The notification area: the top right of the content area, down to the top
/// of the editor. Toasts are drawn only here. It lies inside the content
/// area, so it can never reach the trust margin, the editor or the strip.
#[must_use]
pub fn notification_area(l: &Layout) -> Rect {
    let w = TOAST_COLUMNS.min(l.content.w);
    Rect {
        x: l.content.x + l.content.w - w,
        y: l.content.y,
        w,
        h: l.editor.y.saturating_sub(l.content.y),
    }
}
