//! Where the window looks along the document: at the bottom, following new
//! output, or held at a place the user scrolled to.
//!
//! The document is a flow of blocks, top to bottom, `total` pixels tall, and the
//! content area shows `area` pixels of it. This is the whole model: a
//! position in pixels and one rule, that scrolling up holds the place and
//! reaching the bottom again follows. It knows nothing about blocks, so the
//! renderer, the pointer mapping and a search that has to reveal a row all
//! use the same value.

/// The view's position along the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scroll {
    /// At the bottom: the newest output is in view and stays in view.
    #[default]
    Follow,
    /// Held with the top of the view `top` pixels below the top of the flow.
    /// New output below it does not move the view.
    Pinned(u32),
}

impl Scroll {
    /// The pixel row of the flow at the top of the view, for a flow `total`
    /// pixels tall and a view `area` pixels tall. A flow shorter than the view
    /// starts at the top.
    #[must_use]
    pub fn top(self, total: u32, area: u32) -> u32 {
        let last = total.saturating_sub(area);
        match self {
            Scroll::Follow => last,
            Scroll::Pinned(top) => top.min(last),
        }
    }

    /// Whether the view is at the bottom.
    #[must_use]
    pub fn at_bottom(self, total: u32, area: u32) -> bool {
        self.top(total, area) >= total.saturating_sub(area)
    }

    /// The view moved by `pixels`: up for a positive value, down for a negative
    /// one. Reaching the bottom returns to [`Scroll::Follow`]; the top is a
    /// stop.
    #[must_use]
    pub fn up(self, total: u32, area: u32, pixels: i64) -> Scroll {
        let last = i64::from(total.saturating_sub(area));
        let now = i64::from(self.top(total, area));
        let next = (now - pixels).clamp(0, last);
        if next >= last {
            Scroll::Follow
        } else {
            Scroll::Pinned(u32::try_from(next).unwrap_or(0))
        }
    }

    /// The view after the oldest `pixels` of a block that starts `block_top`
    /// pixels down the flow were dropped from its history. A held view keeps the
    /// text it shows by moving up with the rows that went; once the rows at its
    /// top are themselves gone it holds at the oldest output that is left. A
    /// view above the block, or following the bottom, does not move.
    #[must_use]
    pub fn after_eviction(self, block_top: u32, pixels: u32) -> Scroll {
        match self {
            Scroll::Pinned(top) if top >= block_top => {
                Scroll::Pinned(top.saturating_sub(pixels).max(block_top))
            }
            other => other,
        }
    }

    /// How many of the oldest rows of `old` are gone from `new`, two fingerprints
    /// of a block's history, oldest first: the smallest count after which what
    /// is left of `old` is the start of `new`. Rows are only dropped from the
    /// front and added at the back, so this is how many were dropped. All of
    /// `old` when nothing of it is left.
    #[must_use]
    pub fn rows_evicted(old: &[u64], new: &[u64]) -> usize {
        (0..=old.len())
            .find(|&gone| {
                let left = &old[gone..];
                left.len() <= new.len() && new[..left.len()] == *left
            })
            .unwrap_or(old.len())
    }

    /// The view with its top at `top` pixels of the flow, held; the bottom is
    /// [`Scroll::Follow`].
    #[must_use]
    pub fn to(total: u32, area: u32, top: u32) -> Scroll {
        let last = total.saturating_sub(area);
        if top >= last {
            Scroll::Follow
        } else {
            Scroll::Pinned(top)
        }
    }

    /// The view at the top of the flow.
    #[must_use]
    pub fn to_top(total: u32, area: u32) -> Scroll {
        Scroll::Follow.up(total, area, i64::from(total))
    }

    /// The view moved by a page: `area` less one `row`, so that a line stays in
    /// view across the move. Up for `up`, down otherwise.
    #[must_use]
    pub fn page(self, total: u32, area: u32, row: u32, up: bool) -> Scroll {
        let step = i64::from(area.saturating_sub(row).max(row.max(1)));
        self.up(total, area, if up { step } else { -step })
    }

    /// The thumb of a scroll bar for this view: its top and height, in pixels of
    /// a bar `area` tall, or `None` when the whole flow is in view.
    #[must_use]
    pub fn thumb(self, total: u32, area: u32) -> Option<(u32, u32)> {
        if total <= area || area == 0 {
            return None;
        }
        let t = u64::from(total);
        let a = u64::from(area);
        let height = (a * a / t).clamp(1, a);
        let top = u64::from(self.top(total, area)) * (a - height) / u64::from(total - area);
        Some((
            u32::try_from(top).unwrap_or(0),
            u32::try_from(height).unwrap_or(0),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_dropped_from_the_front_are_counted_by_where_the_old_history_starts_in_the_new() {
        let old = [1, 2, 3, 4, 5];
        assert_eq!(Scroll::rows_evicted(&old, &[1, 2, 3, 4, 5]), 0);
        assert_eq!(
            Scroll::rows_evicted(&old, &[1, 2, 3, 4, 5, 6]),
            0,
            "it grew"
        );
        assert_eq!(Scroll::rows_evicted(&old, &[3, 4, 5, 6, 7]), 2);
        assert_eq!(Scroll::rows_evicted(&old, &[5, 6, 7, 8, 9]), 4);
        assert_eq!(Scroll::rows_evicted(&old, &[6, 7, 8, 9, 10]), 5, "all gone");
        assert_eq!(Scroll::rows_evicted(&[], &[1, 2]), 0);
        // Lines that repeat: the same text is in view whichever count is taken.
        assert_eq!(Scroll::rows_evicted(&[7, 7, 7], &[7, 7, 7]), 0);
    }

    #[test]
    fn a_held_view_moves_up_with_the_rows_that_were_dropped_and_stops_at_the_oldest() {
        let held = Scroll::Pinned(500);
        assert_eq!(held.after_eviction(0, 48), Scroll::Pinned(452));
        assert_eq!(held.after_eviction(0, 500), Scroll::Pinned(0));
        assert_eq!(
            held.after_eviction(0, 9000),
            Scroll::Pinned(0),
            "holds at the oldest"
        );
        assert_eq!(
            held.after_eviction(200, 9000),
            Scroll::Pinned(200),
            "the oldest of a block that starts lower down"
        );
        assert_eq!(held.after_eviction(800, 48), held, "a view above the block");
        assert_eq!(Scroll::Follow.after_eviction(0, 48), Scroll::Follow);
    }

    #[test]
    fn a_short_flow_starts_at_the_top_and_does_not_scroll() {
        assert_eq!(Scroll::Follow.top(100, 400), 0);
        assert_eq!(Scroll::Follow.up(100, 400, 50), Scroll::Follow);
        assert_eq!(Scroll::Follow.thumb(100, 400), None);
    }

    #[test]
    fn scrolling_up_holds_the_place_as_the_flow_grows() {
        let held = Scroll::Follow.up(1000, 400, 200);
        assert_eq!(held, Scroll::Pinned(400));
        assert_eq!(held.top(1000, 400), 400);
        assert_eq!(
            held.top(5000, 400),
            400,
            "new output below does not move it"
        );
        assert!(!held.at_bottom(5000, 400));
    }

    #[test]
    fn reaching_the_bottom_follows_again() {
        let held = Scroll::Follow.up(1000, 400, 200);
        assert_eq!(held.up(1000, 400, -100), Scroll::Pinned(500));
        assert_eq!(held.up(1000, 400, -200), Scroll::Follow);
        assert_eq!(held.up(1000, 400, -10_000), Scroll::Follow);
    }

    #[test]
    fn a_view_set_to_a_place_holds_there_and_the_bottom_follows() {
        assert_eq!(Scroll::to(1000, 400, 250), Scroll::Pinned(250));
        assert_eq!(Scroll::to(1000, 400, 600), Scroll::Follow);
        assert_eq!(Scroll::to(1000, 400, 9000), Scroll::Follow);
    }

    #[test]
    fn the_top_is_a_stop() {
        assert_eq!(Scroll::Follow.up(1000, 400, 10_000), Scroll::Pinned(0));
        assert_eq!(Scroll::to_top(1000, 400), Scroll::Pinned(0));
    }

    #[test]
    fn a_pinned_view_is_clamped_when_the_flow_shrinks() {
        assert_eq!(Scroll::Pinned(900).top(1000, 400), 600);
        assert_eq!(Scroll::Pinned(900).top(300, 400), 0);
    }

    #[test]
    fn a_page_keeps_a_row_in_view() {
        let p = Scroll::Follow.page(2000, 400, 20, true);
        assert_eq!(p, Scroll::Pinned(1600 - 380));
        assert_eq!(p.page(2000, 400, 20, false), Scroll::Follow);
    }

    #[test]
    fn the_thumb_is_at_the_ends_for_the_ends() {
        let (top, height) = Scroll::to_top(2000, 400).thumb(2000, 400).unwrap_or((9, 0));
        assert_eq!(top, 0);
        assert_eq!(height, 80);
        let (top, height) = Scroll::Follow.thumb(2000, 400).unwrap_or((0, 0));
        assert_eq!(top + height, 400);
    }
}
