//! When the bell asks the system for the user's attention.
//!
//! The bell carries no text and no icon: it only makes the system mark the
//! window (the dock icon bounces on macOS, the taskbar entry or the window
//! flashes elsewhere) while the window is not focused. It never draws in the
//! page, and a program that rings the bell in a loop cannot make the mark
//! strobe: a new request waits a minimum time after the last one.

/// The fewest milliseconds between two requests.
pub const MIN_GAP_MS: u64 = 3000;

/// What the window should tell the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do.
    Idle,
    /// Mark the window as wanting attention.
    Request,
    /// The user came back: take the mark away.
    Cancel,
}

/// The state of the attention mark.
#[derive(Debug, Default, Clone, Copy)]
pub struct Attention {
    marked: bool,
    last_ms: Option<u64>,
}

impl Attention {
    /// No mark, nothing asked yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One look at the window: whether the bell rang since the last look, the
    /// time in milliseconds, and whether the window has the focus.
    pub fn step(&mut self, rang: bool, now_ms: u64, focused: bool) -> Step {
        if focused {
            // A bell the user is looking at needs no mark.
            return if std::mem::take(&mut self.marked) {
                Step::Cancel
            } else {
                Step::Idle
            };
        }
        if !rang || self.marked {
            return Step::Idle;
        }
        if self
            .last_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < MIN_GAP_MS)
        {
            return Step::Idle;
        }
        self.marked = true;
        self.last_ms = Some(now_ms);
        Step::Request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bell_in_the_background_asks_once_and_focus_takes_it_back() {
        let mut a = Attention::new();
        assert_eq!(a.step(true, 1000, false), Step::Request);
        assert_eq!(a.step(true, 1100, false), Step::Idle);
        assert_eq!(a.step(false, 1200, true), Step::Cancel);
        assert_eq!(a.step(false, 1300, true), Step::Idle);
    }

    #[test]
    fn a_bell_with_the_focus_asks_for_nothing() {
        let mut a = Attention::new();
        assert_eq!(a.step(true, 0, true), Step::Idle);
    }

    #[test]
    fn a_bell_loop_cannot_strobe_the_mark() {
        let mut a = Attention::new();
        assert_eq!(a.step(true, 10_000, false), Step::Request);
        assert_eq!(a.step(false, 10_100, true), Step::Cancel);
        // Away again and ringing: too soon after the last request.
        assert_eq!(a.step(true, 10_200, false), Step::Idle);
        assert_eq!(a.step(true, 10_000 + MIN_GAP_MS, false), Step::Request);
    }
}
