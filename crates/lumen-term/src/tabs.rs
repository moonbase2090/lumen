//! The sessions in one window and which one owns its input and drawing.

/// A nonempty ordered set of tabs, with one active tab.
pub struct Tabs<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> Tabs<T> {
    /// Start with one active tab.
    pub fn new(first: T) -> Self {
        Self {
            items: vec![first],
            active: 0,
        }
    }

    /// How many tabs the window holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the window has no tabs left.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The active tab's index.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The active tab.
    pub fn active(&self) -> Option<&T> {
        self.items.get(self.active)
    }

    /// The active tab, for a change.
    pub fn active_mut(&mut self) -> Option<&mut T> {
        self.items.get_mut(self.active)
    }

    /// Every tab.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    /// Every tab, for a change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut()
    }

    /// Add a tab and make it active. Returns its index.
    pub fn push(&mut self, item: T) -> usize {
        self.items.push(item);
        self.active = self.items.len() - 1;
        self.active
    }

    /// Make tab `index` active. Returns whether it changed.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.items.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// Select the next tab, wrapping at the end.
    pub fn next(&mut self) -> bool {
        if self.items.len() < 2 {
            return false;
        }
        self.active = (self.active + 1) % self.items.len();
        true
    }

    /// Select the previous tab, wrapping at the start.
    pub fn previous(&mut self) -> bool {
        if self.items.len() < 2 {
            return false;
        }
        self.active = self.active.checked_sub(1).unwrap_or(self.items.len() - 1);
        true
    }

    /// Remove tab `index`. The tab to its right becomes active; at the end,
    /// the new last tab does. Returns the removed tab.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.items.len() {
            return None;
        }
        let removed = self.items.remove(index);
        if self.items.is_empty() {
            self.active = 0;
        } else if index < self.active {
            self.active -= 1;
        } else if self.active >= self.items.len() {
            self.active = self.items.len() - 1;
        }
        Some(removed)
    }

    /// Remove the active tab.
    pub fn remove_active(&mut self) -> Option<T> {
        self.remove(self.active)
    }

    /// The tab under pixel `x` when a strip `width` pixels wide divides its
    /// space equally. Any remainder belongs to the last tabs.
    pub fn hit(&self, x: u32, width: u32) -> Option<usize> {
        if self.items.is_empty() || x >= width || width == 0 {
            return None;
        }
        let count = u32::try_from(self.items.len()).ok()?;
        let index = u64::from(x) * u64::from(count) / u64::from(width);
        usize::try_from(index)
            .ok()
            .filter(|i| *i < self.items.len())
    }
}

#[cfg(test)]
mod tests {
    use super::Tabs;

    #[test]
    fn adding_cycling_and_selecting_choose_one_tab() {
        let mut tabs = Tabs::new("one");
        tabs.push("two");
        tabs.push("three");
        assert_eq!(tabs.active(), Some(&"three"));
        assert!(tabs.next());
        assert_eq!(tabs.active(), Some(&"one"), "next wraps");
        assert!(tabs.previous());
        assert_eq!(tabs.active(), Some(&"three"), "previous wraps");
        assert!(tabs.select(1));
        assert_eq!(tabs.active(), Some(&"two"));
        assert!(!tabs.select(8), "an absent tab changes nothing");
    }

    #[test]
    fn closing_the_active_tab_selects_its_neighbour() {
        let mut tabs = Tabs::new("one");
        tabs.push("two");
        tabs.push("three");
        assert_eq!(tabs.remove_active(), Some("three"));
        assert_eq!(tabs.active(), Some(&"two"));
        assert!(tabs.select(0));
        assert_eq!(tabs.remove_active(), Some("one"));
        assert_eq!(tabs.active(), Some(&"two"));
    }

    #[test]
    fn a_strip_press_finds_the_tab_at_each_edge() {
        let mut tabs = Tabs::new(1);
        tabs.push(2);
        tabs.push(3);
        assert_eq!(tabs.hit(0, 100), Some(0));
        assert_eq!(tabs.hit(33, 100), Some(0));
        assert_eq!(tabs.hit(34, 100), Some(1));
        assert_eq!(tabs.hit(99, 100), Some(2));
        assert_eq!(tabs.hit(100, 100), None);
    }
}
