//! How a pane is named in trust UI: its position and a colour from its id.
//!
//! The rule is `mux.pane-identity` in the spec. A pane is identified by its
//! position (window and pane index) plus a colour derived from its opaque id,
//! and a glyph is added when two colours are close. A "pane changed" marker is
//! shown when a position is reused within 10 seconds. Pane titles, working
//! directories and pane text never appear in trust UI.
//!
//! The last rule is kept by the types: a [`PaneLabel`] is built from three
//! numbers (window, index, id) and cannot hold program text, so nothing a
//! program writes can reach a label.
//!
//! ```compile_fail
//! // A label has no way to take a title or any other text.
//! let _ = lumen_panes::PaneLabel::new(1, 1, 7, "my title");
//! ```

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A position is "reused" when a different pane takes it within this long
/// after the last one left.
pub const REUSE_WINDOW_MS: u64 = 10_000;
/// How long the "pane changed" marker stays up once a position is reused.
pub const MARKER_MS: u64 = 10_000;
/// Two colours are close when their hues are this many degrees apart or less.
pub const CLOSE_HUE_DEG: u32 = 30;
/// The glyphs a pane gets when its colour is close to another's. They are
/// shapes, not letters, so they cannot spell anything.
pub const GLYPHS: [char; 8] = [
    '\u{25b2}', '\u{25a0}', '\u{25cf}', '\u{25c6}', '\u{2605}', '\u{271a}', '\u{25bc}', '\u{25d0}',
];

/// A colour.
pub type Rgb = [u8; 3];

/// The hue, 0 to 359 degrees, an id derives. The same id always gives the same
/// hue.
#[must_use]
pub fn hue_of(pane: u32) -> u32 {
    let mut hash = Sha256::new();
    hash.update(b"lumen pane colour v0");
    hash.update(pane.to_be_bytes());
    let out = hash.finalize();
    u32::from(u16::from_be_bytes([out[0], out[1]])) % 360
}

/// The colour for `hue`: fixed saturation (65%) and lightness (45%), in integer
/// arithmetic so every platform gets the same bytes.
#[must_use]
pub fn colour_of_hue(hue: u32) -> Rgb {
    // Chroma and the lightness offset, in ten-thousandths.
    const CHROMA: u32 = 5850;
    const OFFSET: u32 = 1575;
    let hue = hue % 360;
    let t = hue % 120;
    let second = CHROMA * (60 - t.abs_diff(60)) / 60;
    let (r, g, b) = match hue / 60 {
        0 => (CHROMA, second, 0),
        1 => (second, CHROMA, 0),
        2 => (0, CHROMA, second),
        3 => (0, second, CHROMA),
        4 => (second, 0, CHROMA),
        _ => (CHROMA, 0, second),
    };
    let byte = |v: u32| u8::try_from(((v + OFFSET) * 255 + 5000) / 10_000).unwrap_or(255);
    [byte(r), byte(g), byte(b)]
}

/// How far apart two hues are on the colour wheel, in degrees.
#[must_use]
pub fn hue_distance(a: u32, b: u32) -> u32 {
    let d = a.abs_diff(b) % 360;
    d.min(360 - d)
}

/// What trust UI shows for one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneLabel {
    /// The window the pane is in, counted from 1.
    pub window: u32,
    /// The pane's index in its window, counted from 1.
    pub index: u32,
    /// The pane's opaque id, which only the colour and glyph are derived from.
    pub id: u32,
    /// The hue of the colour.
    pub hue: u32,
    /// The colour.
    pub colour: Rgb,
    /// A glyph, only when another pane's colour is close.
    pub glyph: Option<char>,
}

impl PaneLabel {
    /// A label for pane `id` at `window` and `index`, with no glyph.
    #[must_use]
    pub fn new(window: u32, index: u32, id: u32) -> Self {
        let hue = hue_of(id);
        Self {
            window,
            index,
            id,
            hue,
            colour: colour_of_hue(hue),
            glyph: None,
        }
    }

    /// The label as text: `window 2, pane 3`, with the glyph after it if there
    /// is one. This is the one place the format is written.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!("window {}, pane {}", self.window, self.index);
        if let Some(g) = self.glyph {
            text.push(' ');
            text.push(g);
        }
        text
    }
}

/// Who holds a position, who held it last, and when that one left.
#[derive(Debug, Clone, Copy, Default)]
struct Seat {
    holder: Option<u32>,
    last_holder: Option<u32>,
    left_ms: Option<u64>,
    reused_ms: Option<u64>,
}

/// The panes of a layer, as trust UI names them.
#[derive(Debug, Default)]
pub struct Identity {
    /// Per (window, index): the seat's history.
    seats: BTreeMap<(u32, u32), Seat>,
}

/// The reading-order index of each pane (top to bottom, then left to right),
/// counted from 1.
#[must_use]
pub fn reading_order(panes: &[(u32, crate::Rect)]) -> Vec<(u32, u32)> {
    let mut sorted: Vec<&(u32, crate::Rect)> = panes.iter().collect();
    sorted.sort_by_key(|(id, r)| (r.y, r.x, *id));
    sorted
        .iter()
        .zip(1u32..)
        .map(|((id, _), index)| (*id, index))
        .collect()
}

impl Identity {
    /// An identity table with no panes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A layout took effect at `now_ms`: `panes` are the visible panes of
    /// `window` with their indices. A pane that was in a position and is not
    /// now has left it. A different pane taking a position within
    /// [`REUSE_WINDOW_MS`] of the last one leaving it starts the marker.
    pub fn apply_layout(&mut self, window: u32, panes: &[(u32, u32)], now_ms: u64) {
        // Whoever is not in this layout at their own position has left it.
        for ((w, index), seat) in &mut self.seats {
            if *w != window {
                continue;
            }
            if let Some(holder) = seat.holder
                && !panes.contains(&(holder, *index))
            {
                seat.last_holder = Some(holder);
                seat.holder = None;
                seat.left_ms = Some(now_ms);
            }
        }
        for (id, index) in panes {
            let seat = self.seats.entry((window, *index)).or_default();
            if seat.holder == Some(*id) {
                continue;
            }
            // A different pane than the last one, soon after it left.
            let reused = seat.last_holder.is_some_and(|last| last != *id)
                && seat
                    .left_ms
                    .is_some_and(|left| now_ms.saturating_sub(left) < REUSE_WINDOW_MS);
            seat.reused_ms = reused.then_some(now_ms);
            seat.holder = Some(*id);
        }
    }

    /// The labels of the panes of `window` as of the last layout, with glyphs
    /// added where two colours are close. In index order.
    #[must_use]
    pub fn labels(&self, window: u32) -> Vec<PaneLabel> {
        let mut labels: Vec<PaneLabel> = self
            .seats
            .iter()
            .filter(|((w, _), _)| *w == window)
            .filter_map(|((w, i), seat)| seat.holder.map(|id| PaneLabel::new(*w, *i, id)))
            .collect();
        // Group panes whose colours are close, transitively, and give every
        // pane in a group of two or more its own glyph, in id order.
        let n = labels.len();
        let mut group: Vec<usize> = (0..n).collect();
        for a in 0..n {
            for b in (a + 1)..n {
                if hue_distance(labels[a].hue, labels[b].hue) <= CLOSE_HUE_DEG {
                    let (from, to) = (group[b].max(group[a]), group[b].min(group[a]));
                    for g in &mut group {
                        if *g == from {
                            *g = to;
                        }
                    }
                }
            }
        }
        for root in 0..n {
            let mut members: Vec<usize> = (0..n).filter(|m| group[*m] == root).collect();
            if members.len() < 2 {
                continue;
            }
            members.sort_by_key(|m| labels[*m].id);
            for (slot, m) in members.iter().enumerate() {
                labels[*m].glyph = Some(GLYPHS[slot % GLYPHS.len()]);
            }
        }
        labels
    }

    /// The label of pane `id` in `window`, if it is visible there.
    #[must_use]
    pub fn label(&self, window: u32, id: u32) -> Option<PaneLabel> {
        self.labels(window).into_iter().find(|l| l.id == id)
    }

    /// Whether the "pane changed" marker is up for the pane at `index` in
    /// `window` at `now_ms`: a different pane took the position within
    /// [`REUSE_WINDOW_MS`] of the last one leaving, and [`MARKER_MS`] have not
    /// passed since.
    #[must_use]
    pub fn changed_marker(&self, window: u32, index: u32, now_ms: u64) -> bool {
        self.seats
            .get(&(window, index))
            .and_then(|s| s.reused_ms)
            .is_some_and(|at| now_ms.saturating_sub(at) < MARKER_MS)
    }
}
