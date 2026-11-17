//! The window title: what the program asked for, with the terminal's name.
//!
//! The title goes to the window's own chrome and nowhere else. It is never
//! drawn into the page surface, so a program cannot use it to put words where
//! the terminal's own marks are. The terminal's name always comes last and is
//! always present, so a title cannot stand in for it.

use crate::APP_NAME;

/// The most characters of a program's title that reach the chrome.
pub const MAX_TITLE_CHARS: usize = 256;

/// Characters that change how text is ordered or joined. They are removed so a
/// title cannot reorder the terminal's name or hide where the title ends.
fn is_direction_control(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// The title text for the chrome: direction controls removed, ends trimmed,
/// at most [`MAX_TITLE_CHARS`] characters. `None` when nothing is left.
#[must_use]
pub fn clean(raw: &str) -> Option<String> {
    let kept: String = raw
        .chars()
        .filter(|c| !is_direction_control(*c))
        .take(MAX_TITLE_CHARS)
        .collect();
    let trimmed = kept.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The whole window title for the program's current title, if it has one.
/// An empty, blank or reset title gives the name alone.
#[must_use]
pub fn window_title(program_title: Option<&str>) -> String {
    match program_title.and_then(clean) {
        Some(t) => format!("{t} - {APP_NAME}"),
        None => APP_NAME.to_owned(),
    }
}
