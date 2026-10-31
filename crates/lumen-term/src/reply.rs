//! The terminal's answers to colour queries.
//!
//! A program can ask for the terminal's colours with OSC 4, 10, 11, 12, 17 and
//! 19, and a `?` where the colour would go. The terminal answers from its theme
//! and from nothing else: the reply is the fixed form
//! `ESC ] N ; rgb:RRRR/GGGG/BBBB ESC \` (for OSC 4, `ESC ] 4 ; I ; rgb:...`),
//! built from three numbers and, for OSC 4, an index the parser checked. No byte
//! the program wrote is in a reply.
//!
//! Only the single `?` form is answered. A query that sets a colour, asks for
//! several at once, or has anything else in it gets no reply. At most
//! [`MAX_PER_READ`] are answered for one read of the PTY, and at most
//! [`MAX_PER_WINDOW`] in any sliding [`WINDOW_MS`], so a program cannot make the
//! terminal write to it faster than that.

use crate::raster::{Rgb, Theme, indexed};
use std::collections::VecDeque;

/// Most colour replies for one read of the PTY.
pub const MAX_PER_READ: usize = 8;
/// Most replies of any kind in one sliding window.
pub const MAX_PER_WINDOW: usize = 40;
/// The length of the sliding window, in milliseconds.
pub const WINDOW_MS: u64 = 1000;

/// A colour a program asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourQuery {
    /// OSC 10, 11, 12, 17 or 19 (the number).
    Special(u8),
    /// OSC 4 for one palette entry.
    Indexed(u8),
}

fn number(digits: &[u8]) -> Option<u32> {
    if digits.is_empty()
        || digits.len() > 3
        || !digits.iter().all(u8::is_ascii_digit)
        || (digits.len() > 1 && digits[0] == b'0')
    {
        return None;
    }
    Some(digits.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
}

/// The query in the body of an OSC sequence, if the body is exactly one.
#[must_use]
pub fn parse_query(body: &[u8]) -> Option<ColourQuery> {
    let parts: Vec<&[u8]> = body.split(|b| *b == b';').collect();
    match parts.as_slice() {
        [n, b"?"] => match number(n)? {
            n @ (10 | 11 | 12 | 17 | 19) => u8::try_from(n).ok().map(ColourQuery::Special),
            _ => None,
        },
        [b"4", index, b"?"] => u8::try_from(number(index)?).ok().map(ColourQuery::Indexed),
        _ => None,
    }
}

/// The colour of a query, from the theme. The theme has no cursor or selection
/// colours, so the cursor and the highlight background are the foreground, and
/// the highlight foreground is the background.
fn colour(query: ColourQuery, theme: &Theme) -> Rgb {
    match query {
        ColourQuery::Indexed(n) => indexed(n, theme),
        ColourQuery::Special(11 | 19) => theme.background,
        ColourQuery::Special(_) => theme.foreground,
    }
}

fn format(query: ColourQuery, theme: &Theme) -> Vec<u8> {
    let [r, g, b] = colour(query, theme).map(|v| u16::from(v) * 257);
    let head = match query {
        ColourQuery::Special(n) => format!("{n}"),
        ColourQuery::Indexed(i) => format!("4;{i}"),
    };
    format!("\x1b]{head};rgb:{r:04x}/{g:04x}/{b:04x}\x1b\\").into_bytes()
}

/// Answers colour queries from a theme, within the rate limits.
#[derive(Debug, Clone)]
pub struct Replies {
    theme: Theme,
    sent: VecDeque<u64>,
}

impl Replies {
    /// An answerer that reports `theme`.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            sent: VecDeque::new(),
        }
    }

    /// Report a different theme from now on.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// The bytes to write to the PTY for the queries of one read, at `now_ms`:
    /// the first [`MAX_PER_READ`] of them that the window has room for.
    pub fn answer(&mut self, queries: &[ColourQuery], now_ms: u64) -> Vec<u8> {
        while self
            .sent
            .front()
            .is_some_and(|t| now_ms.saturating_sub(*t) >= WINDOW_MS)
        {
            self.sent.pop_front();
        }
        let room = MAX_PER_WINDOW.saturating_sub(self.sent.len());
        let mut out = Vec::new();
        for query in queries.iter().take(MAX_PER_READ.min(room)) {
            out.extend(format(*query, &self.theme));
            self.sent.push_back(now_ms);
        }
        out
    }
}
