//! The wire payloads of the layout and pane container frames.
//!
//! All integers are big endian. A rectangle is `x`, `y` (signed 32-bit) and
//! `w`, `h` (unsigned 32-bit), in cells of the area the outer terminal gives
//! the multiplexer layer.

/// The largest number of entries a layout may carry.
pub const MAX_PANES: usize = 256;
/// Bytes in a rectangle.
pub const RECT_LEN: usize = 16;
/// Bytes in a layout entry: a pane id and a rectangle.
pub const ENTRY_LEN: usize = 4 + RECT_LEN;
/// Bytes before the content of a pane container.
pub const CONTAINER_PREFIX: usize = 4 + 4 + RECT_LEN;

/// A rectangle in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left column.
    pub x: i32,
    /// Top row.
    pub y: i32,
    /// Width in columns.
    pub w: u32,
    /// Height in rows.
    pub h: u32,
}

impl Rect {
    /// A rectangle is valid if its origin is not negative, it has a size, and
    /// it lies inside the `cols` by `rows` area.
    #[must_use]
    pub fn is_valid(&self, cols: u32, rows: u32) -> bool {
        let (Ok(x), Ok(y)) = (u64::try_from(self.x), u64::try_from(self.y)) else {
            return false;
        };
        self.w > 0
            && self.h > 0
            && x + u64::from(self.w) <= u64::from(cols)
            && y + u64::from(self.h) <= u64::from(rows)
    }

    /// Whether the two rectangles share a cell.
    #[must_use]
    pub fn overlaps(&self, other: &Rect) -> bool {
        let span = |p: i32, n: u32| (i64::from(p), i64::from(p) + i64::from(n));
        let (ax0, ax1) = span(self.x, self.w);
        let (bx0, bx1) = span(other.x, other.w);
        let (ay0, ay1) = span(self.y, self.h);
        let (by0, by1) = span(other.y, other.h);
        ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
    }

    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.x.to_be_bytes());
        out.extend_from_slice(&self.y.to_be_bytes());
        out.extend_from_slice(&self.w.to_be_bytes());
        out.extend_from_slice(&self.h.to_be_bytes());
    }

    fn take(b: &[u8]) -> Option<Rect> {
        let word = |i: usize| -> Option<[u8; 4]> { b.get(i..i + 4)?.try_into().ok() };
        Some(Rect {
            x: i32::from_be_bytes(word(0)?),
            y: i32::from_be_bytes(word(4)?),
            w: u32::from_be_bytes(word(8)?),
            h: u32::from_be_bytes(word(12)?),
        })
    }
}

/// A layout: the visible panes and where they go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The layout generation.
    pub generation: u32,
    /// The visible panes, each with its rectangle.
    pub panes: Vec<(u32, Rect)>,
}

/// A pane container: one pane's content with the generation and rectangle it
/// was drawn for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneContainer {
    /// The pane id.
    pub pane: u32,
    /// The layout generation the container was made for.
    pub generation: u32,
    /// The rectangle.
    pub rect: Rect,
    /// The content type byte, which must be 0x80 or above.
    pub kind: u8,
    /// The content payload.
    pub content: Vec<u8>,
}

/// Why a payload is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// More than [`MAX_PANES`] entries.
    TooManyPanes,
    /// The length does not match the count, or the payload is too short.
    Length,
    /// A pane id appears twice in a layout.
    DuplicatePane,
    /// A rectangle is not valid in the area.
    BadRect,
    /// Two rectangles of a layout overlap.
    Overlap,
    /// The content type byte is missing or below 0x80.
    ContentKind,
}

impl Layout {
    /// The payload bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.generation.to_be_bytes().to_vec();
        out.extend_from_slice(
            &u16::try_from(self.panes.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        for (id, rect) in &self.panes {
            out.extend_from_slice(&id.to_be_bytes());
            rect.put(&mut out);
        }
        out
    }

    /// Parse and check a layout payload for an area of `cols` by `rows`.
    ///
    /// # Errors
    ///
    /// Returns why the layout is malformed.
    pub fn decode(b: &[u8], cols: u32, rows: u32) -> Result<Layout, Malformed> {
        let generation = b
            .get(..4)
            .and_then(|w| <[u8; 4]>::try_from(w).ok())
            .map(u32::from_be_bytes)
            .ok_or(Malformed::Length)?;
        let count = b
            .get(4..6)
            .and_then(|w| <[u8; 2]>::try_from(w).ok())
            .map(u16::from_be_bytes)
            .ok_or(Malformed::Length)?;
        let count = usize::from(count);
        if count > MAX_PANES {
            return Err(Malformed::TooManyPanes);
        }
        let body = b.get(6..).ok_or(Malformed::Length)?;
        if body.len() != count * ENTRY_LEN {
            return Err(Malformed::Length);
        }
        let mut panes: Vec<(u32, Rect)> = Vec::with_capacity(count);
        let (entries, _) = body.as_chunks::<ENTRY_LEN>();
        for entry in entries {
            let id = entry
                .get(..4)
                .and_then(|w| <[u8; 4]>::try_from(w).ok())
                .map(u32::from_be_bytes)
                .ok_or(Malformed::Length)?;
            let rect = entry
                .get(4..)
                .and_then(Rect::take)
                .ok_or(Malformed::Length)?;
            if panes.iter().any(|(other, _)| *other == id) {
                return Err(Malformed::DuplicatePane);
            }
            if !rect.is_valid(cols, rows) {
                return Err(Malformed::BadRect);
            }
            if panes.iter().any(|(_, other)| other.overlaps(&rect)) {
                return Err(Malformed::Overlap);
            }
            panes.push((id, rect));
        }
        Ok(Layout { generation, panes })
    }
}

impl PaneContainer {
    /// The payload bytes: the prefix, then the content type byte and payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.pane.to_be_bytes().to_vec();
        out.extend_from_slice(&self.generation.to_be_bytes());
        self.rect.put(&mut out);
        out.push(self.kind);
        out.extend_from_slice(&self.content);
        out
    }

    /// Parse and check a container payload for an area of `cols` by `rows`.
    ///
    /// # Errors
    ///
    /// Returns why the container is malformed.
    pub fn decode(b: &[u8], cols: u32, rows: u32) -> Result<PaneContainer, Malformed> {
        if b.len() < CONTAINER_PREFIX {
            return Err(Malformed::Length);
        }
        let word = |i: usize| -> Result<u32, Malformed> {
            b.get(i..i + 4)
                .and_then(|w| <[u8; 4]>::try_from(w).ok())
                .map(u32::from_be_bytes)
                .ok_or(Malformed::Length)
        };
        let rect = b
            .get(8..8 + RECT_LEN)
            .and_then(Rect::take)
            .ok_or(Malformed::Length)?;
        if !rect.is_valid(cols, rows) {
            return Err(Malformed::BadRect);
        }
        let kind = *b.get(CONTAINER_PREFIX).ok_or(Malformed::ContentKind)?;
        if kind < lumen_track0::UNSEALED_MIN {
            return Err(Malformed::ContentKind);
        }
        Ok(PaneContainer {
            pane: word(0)?,
            generation: word(4)?,
            rect,
            kind,
            content: b.get(CONTAINER_PREFIX + 1..).unwrap_or_default().to_vec(),
        })
    }
}
