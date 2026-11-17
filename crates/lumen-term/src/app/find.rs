//! The app's find: the bar the user types into, the matches in the document and
//! the place the view goes to for the current one.

use super::App;
use crate::keys::{Key, Mods};
use crate::scroll::Scroll;
use crate::search::{Find, Overlay};

impl App {
    /// Open the find bar with nothing typed. Typing goes to the bar, not the
    /// program, until it is closed. Opening it again keeps what is typed.
    pub fn find_open(&mut self) {
        if self.find.is_none() {
            self.find = Some(Find::new());
            self.view_moves += 1;
        }
    }

    /// Close the find bar: the highlights go and typing goes to the program.
    pub fn find_close(&mut self) {
        if self.find.take().is_some() {
            self.view_moves += 1;
        }
    }

    /// Whether the find bar is open.
    #[must_use]
    pub fn finding(&self) -> bool {
        self.find.is_some()
    }

    /// What is typed in the find bar.
    #[must_use]
    pub fn find_query(&self) -> &str {
        self.find.as_ref().map_or("", Find::query)
    }

    /// How many matches there are.
    #[must_use]
    pub fn find_count(&self) -> usize {
        self.find.as_ref().map_or(0, |f| f.hits().len())
    }

    /// The place of the current match among the matches, from 0.
    #[must_use]
    pub fn find_position(&self) -> Option<usize> {
        self.find.as_ref().and_then(Find::current_index)
    }

    /// The text of the row the current match is in.
    #[must_use]
    pub fn find_current_line(&self) -> Option<String> {
        let hit = *self.find.as_ref()?.current()?;
        let (_, columns, _) = self.content_metrics();
        self.renderer
            .search_rows(self.session.document(), columns)
            .into_iter()
            .find(|r| (r.block, r.row) == (hit.block, hit.row))
            .map(|r| r.text)
    }

    /// What the window draws for the find, if the bar is open.
    pub(super) fn find_overlay(&self) -> Option<Overlay> {
        self.find.as_ref().map(Find::overlay)
    }

    /// Go to the next match, wrapping from the last to the first. Opens the bar
    /// if it is closed. Returns whether the view changed.
    pub fn find_next(&mut self) -> bool {
        self.find_step(true)
    }

    /// Go to the previous match, wrapping from the first to the last.
    pub fn find_previous(&mut self) -> bool {
        self.find_step(false)
    }

    fn find_step(&mut self, forward: bool) -> bool {
        self.find_open();
        self.refresh_find();
        let Some(f) = self.find.as_mut() else {
            return false;
        };
        let moved = if forward {
            f.go_next()
        } else {
            f.go_previous()
        }
        .is_some();
        if moved {
            self.reveal_current();
        }
        moved
    }

    /// Search the document again, for the query as it is now and the output as
    /// it is now. The view does not move.
    pub(super) fn refresh_find(&mut self) {
        let Some(mut f) = self.find.take() else {
            return;
        };
        let (area, columns, total) = self.content_metrics();
        let rows = self.renderer.search_rows(self.session.document(), columns);
        f.refresh(&rows, self.scroll.top(total, area));
        self.find = Some(f);
        self.view_moves += 1;
    }

    /// Bring the current match into view if it is not, in the middle of the
    /// view. The top row of the view is under the bar, so a match there is not
    /// in view.
    fn reveal_current(&mut self) {
        let Some(hit) = self.find.as_ref().and_then(|f| f.current().copied()) else {
            return;
        };
        let (area, _, total) = self.content_metrics();
        let cell = self.renderer.cell().1;
        let top = self.scroll.top(total, area);
        let covered = top.saturating_add(cell);
        let seen =
            hit.top >= covered && hit.top.saturating_add(hit.height) <= top.saturating_add(area);
        if seen {
            return;
        }
        let middle = hit
            .top
            .saturating_add(hit.height / 2)
            .saturating_sub(area / 2);
        let next = Scroll::to(total, area, middle);
        if next != self.scroll {
            self.scroll = next;
        }
        self.view_moves += 1;
    }

    /// The query changed: search again and go to the match the search chose.
    fn find_changed(&mut self) {
        self.refresh_find();
        self.reveal_current();
    }

    /// A key while the find bar is open. Every key is the bar's: none reaches
    /// the program.
    pub(super) fn find_key(&mut self, key: Key, mods: Mods) {
        match key {
            Key::Escape => self.find_close(),
            Key::Enter | Key::KeypadEnter | Key::F(3) => {
                self.find_step(!mods.shift);
            }
            Key::Backspace => {
                if self.find.as_mut().is_some_and(Find::pop) {
                    self.find_changed();
                }
            }
            Key::Char(c) | Key::Keypad(c) if !mods.ctrl && !mods.alt => {
                self.find_text(&c.to_string());
            }
            _ => {}
        }
    }

    /// Text typed or committed while the find bar is open.
    pub(super) fn find_text(&mut self, text: &str) {
        if self.find.as_mut().is_some_and(|f| f.push(text)) {
            self.find_changed();
        }
    }
}
