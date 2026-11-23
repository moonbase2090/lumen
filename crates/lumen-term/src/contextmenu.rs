//! The right-click menu: what it offers, which item is under the pointer or
//! the keys, and what choosing one does.
//!
//! macOS and Windows show the system's own menu for a right click. Where there
//! is none (Linux and the rest), the terminal draws the same menu itself, over
//! everything a program can draw ([`crate::raster`] does the drawing and the
//! hit testing). The items are the same on every system and are the window's
//! own actions, so a choice is carried out by [`crate::App::perform`].

use crate::actions::Action;

/// What the context menu offers: an item is turned off when its action would
/// do nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextState {
    /// Some text is selected, so Copy has something to copy.
    pub can_copy: bool,
    /// The clipboard holds text, so Paste has something to paste.
    pub can_paste: bool,
    /// The menu offers Settings, last: where there is no menu bar to ask for
    /// it (Linux and the other Unix desktops).
    pub settings: bool,
}

/// Whether the menu the terminal draws offers Settings. macOS has it in the
/// menu bar instead, and Windows has no way to open the file yet.
pub(crate) const OFFERS_SETTINGS: bool = cfg!(all(unix, not(target_os = "macos")));

/// One entry of the menu: the action, its label and whether it is on.
pub type Entry = (Action, &'static str, bool);

/// The items of the context menu, in order: the action, its label, and
/// whether it is on. A separator is `None`.
#[must_use]
pub fn context_items(state: ContextState) -> Vec<Option<Entry>> {
    let mut items = vec![
        Some((Action::Copy, "Copy", state.can_copy)),
        Some((Action::Paste, "Paste", state.can_paste)),
        Some((Action::SelectAll, "Select All", true)),
        None,
        Some((Action::ZoomIn, "Zoom In", true)),
        Some((Action::ZoomOut, "Zoom Out", true)),
        Some((Action::ZoomReset, "Actual Size", true)),
    ];
    if state.settings {
        items.extend([None, Some((Action::Settings, "Settings…", true))]);
    }
    items
}

/// A context menu that is open: where it was asked for, what it offers, and
/// which item is highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenu {
    /// The pixel the menu opens at (the pointer when it was asked for).
    pub at: (u32, u32),
    items: Vec<Option<Entry>>,
    hover: Option<usize>,
    first: usize,
}

impl ContextMenu {
    /// A menu opening at pixel `at`, offering what `state` allows.
    #[must_use]
    pub fn new(at: (u32, u32), state: ContextState) -> Self {
        Self {
            at,
            items: context_items(state),
            hover: None,
            first: 0,
        }
    }

    /// The entries in order; a separator is `None`.
    #[must_use]
    pub fn items(&self) -> &[Option<Entry>] {
        &self.items
    }

    /// The highlighted entry, as an index into [`Self::items`].
    #[must_use]
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// The first entry shown when the menu is taller than the window and is
    /// shown a page at a time; 0 otherwise.
    #[must_use]
    pub fn first(&self) -> usize {
        self.first
    }

    /// Show the page that starts at entry `first`.
    pub fn set_first(&mut self, first: usize) {
        self.first = first;
    }

    /// Whether the entry at `index` can be chosen.
    #[must_use]
    pub fn is_on(&self, index: usize) -> bool {
        matches!(self.items.get(index), Some(Some((_, _, true))))
    }

    /// Highlight the entry at `index` (or none). Only an entry that is on can
    /// be highlighted. Returns whether the highlight changed.
    pub fn set_hover(&mut self, index: Option<usize>) -> bool {
        let index = index.filter(|&i| self.is_on(i));
        let changed = index != self.hover;
        self.hover = index;
        changed
    }

    /// Move the highlight to the next entry that is on, wrapping, or to the
    /// previous one for `forward == false`. With nothing highlighted, forward
    /// starts at the first and backward at the last.
    pub fn step(&mut self, forward: bool) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let mut at = self.hover;
        for _ in 0..n {
            let next = match (at, forward) {
                (None, true) => 0,
                (None, false) => n - 1,
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
            };
            if self.is_on(next) {
                self.hover = Some(next);
                return;
            }
            at = Some(next);
        }
    }

    /// The action of the highlighted entry, if one is highlighted.
    #[must_use]
    pub fn chosen(&self) -> Option<Action> {
        match self.items.get(self.hover?) {
            Some(Some((action, _, true))) => Some(*action),
            _ => None,
        }
    }

    /// The action of the entry at `index`, if it is on.
    #[must_use]
    pub fn action_at(&self, index: usize) -> Option<Action> {
        match self.items.get(index) {
            Some(Some((action, _, true))) => Some(*action),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(can_copy: bool, can_paste: bool) -> ContextMenu {
        ContextMenu::new(
            (10, 10),
            ContextState {
                can_copy,
                can_paste,
                settings: false,
            },
        )
    }

    #[test]
    fn keys_skip_entries_that_are_off_and_the_separator_and_wrap() {
        let mut m = menu(false, false);
        m.step(true);
        assert_eq!(m.chosen(), Some(Action::SelectAll));
        m.step(true);
        assert_eq!(m.chosen(), Some(Action::ZoomIn));
        m.step(true);
        m.step(true);
        assert_eq!(m.chosen(), Some(Action::ZoomReset));
        m.step(true);
        assert_eq!(
            m.chosen(),
            Some(Action::SelectAll),
            "wraps past Copy and Paste"
        );
        m.step(false);
        assert_eq!(m.chosen(), Some(Action::ZoomReset), "backwards wraps");
    }

    #[test]
    fn backward_from_nothing_starts_at_the_last_entry() {
        let mut m = menu(true, true);
        m.step(false);
        assert_eq!(m.chosen(), Some(Action::ZoomReset));
    }

    #[test]
    fn only_an_entry_that_is_on_can_be_highlighted() {
        let mut m = menu(false, true);
        assert!(!m.set_hover(Some(0)), "Copy is off");
        assert_eq!(m.hover(), None);
        assert!(m.set_hover(Some(1)));
        assert_eq!(m.chosen(), Some(Action::Paste));
        assert!(
            m.set_hover(Some(3)),
            "moving onto the separator clears the highlight"
        );
        assert_eq!(m.hover(), None);
        assert!(!m.set_hover(Some(3)), "and nothing changes the next time");
    }

    #[test]
    fn settings_comes_last_after_a_separator_and_the_keys_reach_it() {
        let mut m = ContextMenu::new(
            (10, 10),
            ContextState {
                can_copy: false,
                can_paste: false,
                settings: true,
            },
        );
        let items = m.items();
        assert_eq!(items[items.len() - 2], None);
        assert_eq!(
            items[items.len() - 1],
            Some((Action::Settings, "Settings…", true))
        );
        m.step(false);
        assert_eq!(m.chosen(), Some(Action::Settings), "backwards starts there");
        m.step(true);
        assert_eq!(m.chosen(), Some(Action::SelectAll), "and forwards wraps");
    }

    #[test]
    fn nothing_is_chosen_until_something_is_highlighted() {
        assert_eq!(menu(true, true).chosen(), None);
        assert_eq!(menu(true, true).action_at(0), Some(Action::Copy));
        assert_eq!(menu(false, true).action_at(0), None);
    }
}
