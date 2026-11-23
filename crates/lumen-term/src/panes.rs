//! The panes of one tab: the area is split side by side or stacked, each part
//! holds a session of its own, and one of them has the focus.
//!
//! The layout is pure: [`Panes`] holds whatever a pane is (the window uses an
//! [`App`](crate::App)) and says where each one goes in an area, so it is
//! tested without a window. The panes are kept in reading order, left to right
//! and top to bottom, and focus moves along that order.

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
}

enum Node<T> {
    /// What a tree is while it is being rebuilt; never seen from outside.
    Empty,
    Leaf(T),
    Split {
        split: Split,
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
                first,
                second,
            } => {
                let (a, line, b) = divide(area, *split, gap);
                first.place(a, gap, out);
                out.dividers.push(line);
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
                first,
                second,
            } => Self::Split {
                split,
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
}

/// Cut `area` in two along `split`, leaving `gap` pixels for the divider.
fn divide(area: Rect, split: Split, gap: u32) -> (Rect, Rect, Rect) {
    match split {
        Split::Right => {
            let first = area.w.saturating_sub(gap) / 2;
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
            let first = area.h.saturating_sub(gap) / 2;
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
        let (first, _, second) = divide(*rect, how, gap);
        fits(first.w, first.h) && fits(second.w, second.h)
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
