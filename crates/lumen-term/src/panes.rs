//! The panes of one tab: the area is split side by side or stacked, each part
//! holds a session of its own, and one of them has the focus.
//!
//! The layout is pure: [`Panes`] holds whatever a pane is (the window uses an
//! [`App`](crate::App)) and says where each one goes in an area, so it is
//! tested without a window. The panes are kept in reading order, left to right
//! and top to bottom, and focus moves along that order.
//!
//! A split starts at half and keeps its share of the space when the area
//! changes size. Dragging a divider changes the share of the split it belongs
//! to, as far as every pane on both sides stays acceptable.

/// Which way a split divides a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Two panes side by side: a vertical divider.
    Right,
    /// Two panes one above the other: a horizontal divider.
    Down,
}

/// A rectangle of the area, in pixels from its top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl Rect {
    /// Whether the point is inside.
    #[must_use]
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Where every pane goes, and the dividers between them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// Each pane's rectangle, in reading order.
    pub panes: Vec<Rect>,
    /// The dividers drawn between panes.
    pub dividers: Vec<Rect>,
    /// For each divider, in the same order, the split it belongs to and the
    /// area that split divides.
    pub splits: Vec<(Split, Rect)>,
}

impl Layout {
    /// The pane at the area's top right corner: of the panes on the top edge,
    /// the one furthest right. The terminal's notices about ignored files go
    /// there, where a window of one pane has its notification area.
    #[must_use]
    pub fn top_right(&self) -> usize {
        let top = self.panes.iter().map(|r| r.y).min().unwrap_or(0);
        self.panes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.y == top)
            .max_by_key(|(_, r)| r.x + r.w)
            .map_or(0, |(i, _)| i)
    }
}

/// A whole share of a split's space: a split's share is the part of it, in
/// these units, that goes to its first side.
const SHARE_WHOLE: u32 = 10_000;
/// The share a new split starts at.
const SHARE_HALF: u32 = SHARE_WHOLE / 2;

enum Node<T> {
    /// What a tree is while it is being rebuilt; never seen from outside.
    Empty,
    Leaf(T),
    Split {
        split: Split,
        share: u32,
        first: Box<Node<T>>,
        second: Box<Node<T>>,
    },
}

/// A nonempty set of panes with one focused.
pub struct Panes<T> {
    root: Node<T>,
    focus: usize,
}

impl<T> Node<T> {
    fn count(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => first.count() + second.count(),
        }
    }

    fn walk<'a>(&'a self, out: &mut Vec<&'a T>) {
        match self {
            Self::Empty => {}
            Self::Leaf(t) => out.push(t),
            Self::Split { first, second, .. } => {
                first.walk(out);
                second.walk(out);
            }
        }
    }

    fn walk_mut<'a>(&'a mut self, out: &mut Vec<&'a mut T>) {
        match self {
            Self::Empty => {}
            Self::Leaf(t) => out.push(t),
            Self::Split { first, second, .. } => {
                first.walk_mut(out);
                second.walk_mut(out);
            }
        }
    }

    fn place(&self, area: Rect, gap: u32, out: &mut Layout) {
        match self {
            Self::Empty => {}
            Self::Leaf(_) => out.panes.push(area),
            Self::Split {
                split,
                share,
                first,
                second,
            } => {
                let (a, line, b) = divide(area, *split, gap, *share);
                first.place(a, gap, out);
                out.dividers.push(line);
                out.splits.push((*split, area));
                second.place(b, gap, out);
            }
        }
    }

    /// Put `new` beside the leaf `target` (counting from `seen`), splitting it.
    fn insert(self, target: usize, seen: &mut usize, new: &mut Option<T>, how: Split) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Leaf(t) => {
                let here = *seen == target;
                *seen += 1;
                match new.take() {
                    Some(n) if here => Self::Split {
                        split: how,
                        share: SHARE_HALF,
                        first: Box::new(Self::Leaf(t)),
                        second: Box::new(Self::Leaf(n)),
                    },
                    other => {
                        *new = other;
                        Self::Leaf(t)
                    }
                }
            }
            Self::Split {
                split,
                share,
                first,
                second,
            } => Self::Split {
                split,
                share,
                first: Box::new(first.insert(target, seen, new, how)),
                second: Box::new(second.insert(target, seen, new, how)),
            },
        }
    }

    fn leaves(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => first.leaves() + second.leaves(),
        }
    }

    /// The leaf that takes the space of leaf `target` (counted from `base`)
    /// when it goes: the nearest one in the sibling that collapses into it.
    fn heir(&self, target: usize, base: usize) -> Option<usize> {
        let Self::Split { first, second, .. } = self else {
            return None;
        };
        let before = first.leaves();
        if target < base + before {
            match **first {
                Self::Leaf(_) => Some(base + before),
                _ => first.heir(target, base),
            }
        } else {
            match **second {
                Self::Leaf(_) => Some(base + before - 1),
                _ => second.heir(target, base + before),
            }
        }
    }

    /// Take out the leaf `target`; what is left, if anything, and the leaf.
    fn remove(self, target: usize, seen: &mut usize) -> (Option<Self>, Option<T>) {
        match self {
            Self::Empty => (None, None),
            Self::Leaf(t) => {
                let here = *seen == target;
                *seen += 1;
                if here {
                    (None, Some(t))
                } else {
                    (Some(Self::Leaf(t)), None)
                }
            }
            Self::Split {
                split,
                share,
                first,
                second,
            } => {
                let (a, taken_a) = first.remove(target, seen);
                let (b, taken_b) = second.remove(target, seen);
                let taken = taken_a.or(taken_b);
                match (a, b) {
                    (Some(a), Some(b)) => (
                        Some(Self::Split {
                            split,
                            share,
                            first: Box::new(a),
                            second: Box::new(b),
                        }),
                        taken,
                    ),
                    (one, other) => (one.or(other), taken),
                }
            }
        }
    }

    /// The share of the split whose divider is `target` in layout order
    /// (counting from `seen`), for a change.
    fn share_mut(&mut self, target: usize, seen: &mut usize) -> Option<&mut u32> {
        let Self::Split {
            share,
            first,
            second,
            ..
        } = self
        else {
            return None;
        };
        if let Some(found) = first.share_mut(target, seen) {
            return Some(found);
        }
        let here = *seen == target;
        *seen += 1;
        if here {
            return Some(share);
        }
        second.share_mut(target, seen)
    }
}

/// The part of `avail` pixels that a split of `share` gives its first side.
fn first_part(avail: u32, share: u32) -> u32 {
    u32::try_from(u64::from(avail) * u64::from(share.min(SHARE_WHOLE)) / u64::from(SHARE_WHOLE))
        .unwrap_or(avail)
}

/// Cut `area` in two along `split`, `share` of it to the first side, leaving
/// `gap` pixels for the divider.
fn divide(area: Rect, split: Split, gap: u32, share: u32) -> (Rect, Rect, Rect) {
    match split {
        Split::Right => {
            let first = first_part(area.w.saturating_sub(gap), share);
            let second = area.w.saturating_sub(gap).saturating_sub(first);
            (
                Rect { w: first, ..area },
                Rect {
                    x: area.x + first,
                    w: gap.min(area.w.saturating_sub(first)),
                    ..area
                },
                Rect {
                    x: area.x + first + gap.min(area.w.saturating_sub(first)),
                    w: second,
                    ..area
                },
            )
        }
        Split::Down => {
            let first = first_part(area.h.saturating_sub(gap), share);
            let second = area.h.saturating_sub(gap).saturating_sub(first);
            (
                Rect { h: first, ..area },
                Rect {
                    y: area.y + first,
                    h: gap.min(area.h.saturating_sub(first)),
                    ..area
                },
                Rect {
                    y: area.y + first + gap.min(area.h.saturating_sub(first)),
                    h: second,
                    ..area
                },
            )
        }
    }
}

impl<T> Panes<T> {
    /// One pane, focused.
    pub fn new(first: T) -> Self {
        Self {
            root: Node::Leaf(first),
            focus: 0,
        }
    }

    /// How many panes there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.root.count()
    }

    /// Always false: a set of panes keeps at least one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The focused pane's place in reading order.
    #[must_use]
    pub fn focus_index(&self) -> usize {
        self.focus
    }

    /// Every pane, in reading order.
    #[must_use]
    pub fn all(&self) -> Vec<&T> {
        let mut out = Vec::new();
        self.root.walk(&mut out);
        out
    }

    /// Every pane, in reading order, for a change.
    pub fn all_mut(&mut self) -> Vec<&mut T> {
        let mut out = Vec::new();
        self.root.walk_mut(&mut out);
        out
    }

    /// The focused pane.
    #[must_use]
    pub fn focused(&self) -> &T {
        let all = self.all();
        all[self.focus.min(all.len() - 1)]
    }

    /// The focused pane, for a change.
    pub fn focused_mut(&mut self) -> &mut T {
        let at = self.focus;
        let mut all = self.all_mut();
        let last = all.len() - 1;
        all.swap_remove(at.min(last))
    }

    /// The pane at `index` in reading order, for a change.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let mut all = self.all_mut();
        (index < all.len()).then(|| all.swap_remove(index))
    }

    /// Where each pane goes in a `width` by `height` area, with dividers `gap`
    /// pixels thick.
    #[must_use]
    pub fn layout(&self, width: u32, height: u32, gap: u32) -> Layout {
        let mut out = Layout::default();
        self.root.place(
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            gap,
            &mut out,
        );
        out
    }

    /// Whether the focused pane can be split `how` and leave both halves
    /// acceptable to `fits`, which is given a half's size in pixels.
    #[must_use]
    pub fn can_split(
        &self,
        how: Split,
        (width, height, gap): (u32, u32, u32),
        fits: impl Fn(u32, u32) -> bool,
    ) -> bool {
        let layout = self.layout(width, height, gap);
        let Some(rect) = layout.panes.get(self.focus) else {
            return false;
        };
        let (first, _, second) = divide(*rect, how, gap, SHARE_HALF);
        fits(first.w, first.h) && fits(second.w, second.h)
    }

    /// The divider under the point `(x, y)` of a `width` by `height` area with
    /// dividers `gap` pixels thick, counting `grab` pixels on either side of it
    /// as on it, so a thin line can be taken hold of.
    #[must_use]
    pub fn divider_at(
        &self,
        (width, height, gap): (u32, u32, u32),
        x: u32,
        y: u32,
        grab: u32,
    ) -> Option<usize> {
        let layout = self.layout(width, height, gap);
        layout
            .dividers
            .iter()
            .zip(&layout.splits)
            .position(|(line, (split, _))| {
                let near =
                    |at: u32, start: u32, len: u32| at + grab >= start && at < start + len + grab;
                match split {
                    Split::Right => near(x, line.x, line.w) && y >= line.y && y < line.y + line.h,
                    Split::Down => near(y, line.y, line.h) && x >= line.x && x < line.x + line.w,
                }
            })
    }

    /// Move divider `index` towards `at`, a pixel of the area along the
    /// divider's axis (across for a vertical one, down for a horizontal one),
    /// as far as every pane stays acceptable to `fits`, which is given a pane
    /// and the size in pixels it would have. Whether the divider moved.
    pub fn drag(
        &mut self,
        index: usize,
        at: u32,
        (width, height, gap): (u32, u32, u32),
        fits: impl Fn(&T, u32, u32) -> bool,
    ) -> bool {
        let layout = self.layout(width, height, gap);
        let Some(&(split, area)) = layout.splits.get(index) else {
            return false;
        };
        let (start, len) = match split {
            Split::Right => (area.x, area.w),
            Split::Down => (area.y, area.h),
        };
        let avail = len.saturating_sub(gap);
        if avail == 0 {
            return false;
        }
        // The divider's middle follows the pointer.
        let first = at.saturating_sub(start).saturating_sub(gap / 2).min(avail);
        let target =
            u32::try_from((u64::from(first) * u64::from(SHARE_WHOLE)).div_ceil(u64::from(avail)))
                .unwrap_or(SHARE_WHOLE)
                .min(SHARE_WHOLE);
        let Some(now) = self.share(index) else {
            return false;
        };
        if target == now {
            return false;
        }
        let ok = |panes: &mut Self, share: u32| {
            panes.set_share(index, share);
            let layout = panes.layout(width, height, gap);
            panes
                .all()
                .into_iter()
                .zip(layout.panes)
                .all(|(pane, r)| fits(pane, r.w, r.h))
        };
        if ok(self, target) {
            return true;
        }
        if !ok(self, now) {
            // Already too small, as after the area shrank: no drag makes it
            // worse, and none is taken that does not fix it.
            return false;
        }
        // Every pane fits at `now` and some pane does not at `target`; each
        // pane only grows or only shrinks on the way, so the last share that
        // fits is found by halving.
        let (mut good, mut bad) = (now, target);
        while good.abs_diff(bad) > 1 {
            let mid = if good < bad {
                good + (bad - good) / 2
            } else {
                bad + (good - bad) / 2
            };
            if ok(self, mid) {
                good = mid;
            } else {
                bad = mid;
            }
        }
        self.set_share(index, good);
        good != now
    }

    fn share(&mut self, index: usize) -> Option<u32> {
        self.root.share_mut(index, &mut 0).map(|s| *s)
    }

    fn set_share(&mut self, index: usize, share: u32) {
        if let Some(s) = self.root.share_mut(index, &mut 0) {
            *s = share;
        }
    }

    /// Split the focused pane: `new` takes the second half and the focus.
    pub fn split(&mut self, new: T, how: Split) {
        let root = std::mem::replace(&mut self.root, Node::Empty);
        let mut seen = 0;
        let mut slot = Some(new);
        self.root = root.insert(self.focus, &mut seen, &mut slot, how);
        self.focus += 1;
    }

    /// Take pane `index` out. The neighbour that takes its space is focused
    /// when the removed pane had the focus. `None` for the last pane or an
    /// index past the end.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        let count = self.len();
        if count < 2 || index >= count {
            return None;
        }
        let heir = self.root.heir(index, 0).unwrap_or(0);
        let root = std::mem::replace(&mut self.root, Node::Empty);
        let mut seen = 0;
        let (rest, taken) = root.remove(index, &mut seen);
        self.root = rest.unwrap_or(Node::Empty);
        let shifted = |i: usize| if i > index { i - 1 } else { i };
        self.focus = if self.focus == index {
            shifted(heir)
        } else {
            shifted(self.focus)
        };
        taken
    }

    /// Focus pane `index`. Whether the focus moved.
    pub fn focus(&mut self, index: usize) -> bool {
        if index >= self.len() || index == self.focus {
            return false;
        }
        self.focus = index;
        true
    }

    /// Focus the next pane, wrapping at the end. Whether the focus moved.
    pub fn focus_next(&mut self) -> bool {
        let n = self.len();
        self.focus((self.focus + 1) % n)
    }

    /// Focus the previous pane, wrapping at the start. Whether the focus moved.
    pub fn focus_previous(&mut self) -> bool {
        let n = self.len();
        self.focus((self.focus + n - 1) % n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(p: &Panes<&'static str>) -> Vec<&'static str> {
        p.all().into_iter().copied().collect()
    }

    #[test]
    fn closing_a_pane_focuses_the_pane_that_takes_its_space() {
        // a | (b over c); close c: b grows, not a.
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Down);
        assert_eq!(p.focus_index(), 2);
        assert_eq!(p.remove(2), Some("c"));
        assert_eq!(p.focused(), &"b");
        // (a over b) | c; close a with focus on a: b grows.
        let mut p = Panes::new("a");
        p.split("b", Split::Down);
        p.split("c", Split::Right);
        p.focus(0);
        p.split("d", Split::Down);
        p.focus(0);
        assert_eq!(p.remove(0), Some("a"));
        assert_eq!(p.focused(), &"d");
    }

    #[test]
    fn a_split_keeps_both_halves_at_the_minimum_on_both_axes() {
        let p = Panes::new("a");
        let min = |w: u32, h: u32| w >= 20 && h >= 5;
        assert!(p.can_split(Split::Right, (100, 20, 2), min));
        assert!(!p.can_split(Split::Right, (100, 4, 2), min), "too short");
        assert!(p.can_split(Split::Down, (100, 20, 2), min));
        assert!(!p.can_split(Split::Down, (19, 20, 2), min), "too narrow");
    }

    #[test]
    fn the_top_right_pane_is_on_the_top_edge_and_furthest_right() {
        let mut p = Panes::new("a");
        assert_eq!(
            p.layout(100, 50, 2).top_right(),
            0,
            "one pane is the corner"
        );
        p.split("b", Split::Right);
        assert_eq!(
            p.layout(100, 50, 2).top_right(),
            1,
            "side by side: the right one"
        );
        p.split("c", Split::Down);
        // a | b over c: b still holds the corner, c is below it.
        assert_eq!(names(&p), ["a", "b", "c"]);
        assert_eq!(p.layout(100, 50, 2).top_right(), 1);
        p.focus(0);
        p.split("d", Split::Down);
        // a over d | b over c: reading order puts d before b.
        assert_eq!(names(&p), ["a", "d", "b", "c"]);
        assert_eq!(p.layout(100, 50, 2).top_right(), 2);
        let mut q = Panes::new("a");
        q.split("b", Split::Down);
        assert_eq!(q.layout(100, 50, 2).top_right(), 0, "stacked: the top one");
    }

    #[test]
    fn one_pane_fills_the_area() {
        let p = Panes::new("a");
        let l = p.layout(100, 50, 2);
        assert_eq!(
            l.panes,
            vec![Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 50
            }]
        );
        assert_eq!(l.dividers, vec![]);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn a_split_right_puts_the_new_pane_second_and_focuses_it() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        assert_eq!(names(&p), ["a", "b"]);
        assert_eq!(*p.focused(), "b");
        let l = p.layout(101, 50, 3);
        assert_eq!(
            l.panes[0],
            Rect {
                x: 0,
                y: 0,
                w: 49,
                h: 50
            }
        );
        assert_eq!(
            l.dividers[0],
            Rect {
                x: 49,
                y: 0,
                w: 3,
                h: 50
            }
        );
        assert_eq!(
            l.panes[1],
            Rect {
                x: 52,
                y: 0,
                w: 49,
                h: 50
            }
        );
    }

    #[test]
    fn a_split_down_stacks_and_the_panes_and_dividers_tile_the_area() {
        let mut p = Panes::new("a");
        p.split("b", Split::Down);
        p.focus(0);
        p.split("c", Split::Right);
        assert_eq!(names(&p), ["a", "c", "b"]);
        let l = p.layout(200, 101, 1);
        let area: u32 = l.panes.iter().chain(&l.dividers).map(|r| r.w * r.h).sum();
        assert_eq!(area, 200 * 101, "panes and dividers cover the area exactly");
        for (i, a) in l.panes.iter().enumerate() {
            for b in &l.panes[i + 1..] {
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn focus_moves_in_reading_order_and_wraps() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Down);
        assert_eq!(*p.focused(), "c");
        assert!(p.focus_next());
        assert_eq!(*p.focused(), "a");
        assert!(p.focus_previous());
        assert_eq!(*p.focused(), "c");
        let mut one = Panes::new("x");
        assert!(!one.focus_next() && !one.focus_previous());
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_neighbour() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Down);
        assert_eq!(p.remove(2), Some("c"));
        assert_eq!(names(&p), ["a", "b"]);
        assert_eq!(*p.focused(), "b", "the neighbour is focused");
        let l = p.layout(100, 40, 2);
        assert_eq!(l.panes[1].h, 40, "b has the full height again");
        assert_eq!(p.remove(0), Some("a"));
        assert_eq!(p.len(), 1);
        assert_eq!(p.layout(100, 40, 2).panes[0].w, 100);
        assert_eq!(p.remove(0), None, "the last pane is kept");
    }

    #[test]
    fn closing_a_pane_before_the_focus_keeps_the_focus_on_the_same_pane() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Right);
        assert_eq!(*p.focused(), "c");
        assert_eq!(p.remove(0), Some("a"));
        assert_eq!(*p.focused(), "c");
        assert_eq!(p.remove(7), None);
    }

    #[test]
    fn a_pane_too_small_is_not_split() {
        let p = Panes::new("a");
        assert!(p.can_split(Split::Right, (100, 40, 2), |w, h| w >= 49 && h >= 10));
        assert!(!p.can_split(Split::Right, (100, 40, 2), |w, h| w >= 50 && h >= 10));
        assert!(p.can_split(Split::Down, (100, 40, 2), |w, h| w >= 10 && h >= 19));
        assert!(!p.can_split(Split::Down, (100, 40, 2), |w, h| w >= 10 && h >= 20));
    }

    /// The panes and dividers cover the area exactly, with no overlap.
    fn tiles(l: &Layout, width: u32, height: u32) -> bool {
        let area: u32 = l.panes.iter().chain(&l.dividers).map(|r| r.w * r.h).sum();
        let apart = |a: &Rect, b: &Rect| {
            a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
        };
        let all: Vec<&Rect> = l.panes.iter().chain(&l.dividers).collect();
        area == width * height
            && all
                .iter()
                .enumerate()
                .all(|(i, a)| all[i + 1..].iter().all(|b| apart(a, b)))
    }

    #[test]
    fn dragging_a_divider_moves_it_live_and_the_panes_still_tile_the_area() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        let fits = |_: &&str, w: u32, h: u32| w >= 20 && h >= 5;
        // The divider's middle follows the pointer.
        assert!(p.drag(0, 31, (101, 40, 2), fits));
        let l = p.layout(101, 40, 2);
        assert_eq!((l.panes[0].w, l.dividers[0].x, l.panes[1].w), (30, 30, 69));
        assert!(tiles(&l, 101, 40));
        assert!(!p.drag(0, 31, (101, 40, 2), fits), "already there");
        // The share is kept when the area changes size, as on a window resize
        // or a zoom that changes the strip above the panes.
        let l = p.layout(202, 33, 4);
        assert_eq!(l.panes[0].w, 60, "30 of 99 is 60 of 198");
        assert!(tiles(&l, 202, 33));
        // The focus is not the drag's to change.
        assert_eq!(*p.focused(), "b");
    }

    #[test]
    fn a_drag_stops_where_a_pane_on_either_side_would_go_below_its_minimum() {
        // a | (b over c): the outer divider holds back for b and c together.
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Down);
        let fits = |_: &&str, w: u32, h: u32| w >= 20 && h >= 5;
        let area = (100, 40, 2);
        // Dividers are in layout order: the vertical one first, then the one
        // between b and c.
        assert!(p.drag(0, 0, area, fits), "towards the left edge");
        let l = p.layout(100, 40, 2);
        assert_eq!(l.panes[0].w, 20, "a stops at the minimum");
        assert!(p.drag(0, 99, area, fits), "towards the right edge");
        let l = p.layout(100, 40, 2);
        assert_eq!(
            (l.panes[1].w, l.panes[2].w),
            (20, 20),
            "b and c stop together"
        );
        assert!(tiles(&l, 100, 40));
        // The inner divider, stacked: c cannot be made shorter than 5.
        assert!(p.drag(1, 39, area, fits));
        assert_eq!(p.layout(100, 40, 2).panes[2].h, 5);
        // An area already too small for the minimum takes no drag at all.
        assert!(!p.drag(0, 50, (30, 40, 2), fits));
    }

    #[test]
    fn a_press_on_a_divider_takes_hold_of_it_and_a_press_in_a_pane_does_not() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        p.split("c", Split::Down);
        let area = (100, 40, 2);
        // The vertical divider is at 49 and 50; two pixels either side count.
        assert_eq!(p.divider_at(area, 49, 10, 2), Some(0));
        assert_eq!(p.divider_at(area, 47, 10, 2), Some(0));
        assert_eq!(p.divider_at(area, 52, 10, 2), Some(0));
        assert_eq!(p.divider_at(area, 46, 10, 2), None, "inside a");
        assert_eq!(p.divider_at(area, 53, 10, 2), None, "inside b");
        // The horizontal one runs only across b and c.
        let line = p.layout(100, 40, 2).dividers[1];
        assert_eq!(p.divider_at(area, 75, line.y, 2), Some(1));
        assert_eq!(
            p.divider_at(area, 20, line.y, 2),
            None,
            "a has no divider there"
        );
    }

    #[test]
    fn a_pixel_finds_its_pane_and_a_divider_is_no_pane() {
        let mut p = Panes::new("a");
        p.split("b", Split::Right);
        let l = p.layout(100, 40, 2);
        let at = |x: u32, y: u32| l.panes.iter().position(|r| r.contains(x, y));
        assert_eq!(at(0, 0), Some(0));
        assert_eq!(at(60, 39), Some(1));
        assert_eq!(at(l.dividers[0].x, 5), None);
        assert_eq!(at(100, 5), None);
    }
}
