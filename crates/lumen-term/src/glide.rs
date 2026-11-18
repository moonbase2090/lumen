//! A scroll that eases to where it is going instead of jumping there.
//!
//! A wheel step asks the view to move some pixels. The glide keeps what is left
//! of that move and gives back a share of it for each frame, so the view starts
//! quickly and settles: each step of time covers a fixed fraction of the
//! distance that remains. It knows nothing about the view; the app asks it how
//! far to move now and moves the view by that.

/// The time, in milliseconds, in which the distance left halves.
pub const HALF_LIFE_MS: f64 = 55.0;

/// The most pixels a glide holds. A wheel spun hard asks for more than this and
/// the view stops where this much would take it, rather than coasting for
/// seconds after the wheel has stopped.
pub const MAX_PENDING: i64 = 6000;

/// The longest gap between two steps that is counted, in milliseconds. A window
/// that was not drawn for a while does not jump on its first frame back.
pub const MAX_STEP_MS: u64 = 100;

/// Pixels still to move, and the clock of the last step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Glide {
    /// Pixels left: positive for up, as the view's own moves are.
    left: f64,
    /// When the last step was taken, in the caller's milliseconds.
    last_ms: Option<u64>,
}

impl Glide {
    /// A glide with nothing to move.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            left: 0.0,
            last_ms: None,
        }
    }

    /// Ask for `pixels` more of movement, up for a positive number. A move the
    /// other way takes from what is left first, so a reversed wheel turns the
    /// view round at once. Returns the pixels now left, rounded.
    pub fn add(&mut self, pixels: i64) {
        // The sum of two bounded numbers, kept inside the bound.
        #[allow(clippy::cast_precision_loss, reason = "a small pixel count")]
        let next = self.left + pixels as f64;
        #[allow(clippy::cast_precision_loss, reason = "a small pixel count")]
        let cap = MAX_PENDING as f64;
        self.left = if next.is_finite() {
            next.clamp(-cap, cap)
        } else {
            0.0
        };
        if self.left.abs() < 1.0 {
            self.left = 0.0;
        }
    }

    /// Drop what is left: the view moved some other way, or has nowhere to go.
    pub fn cancel(&mut self) {
        self.left = 0.0;
        self.last_ms = None;
    }

    /// Whether there is movement left to give.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.left != 0.0
    }

    /// The whole pixels to move at `now_ms`, up for positive. Zero when nothing
    /// is left. The first step after a rest moves for one frame's time, so a
    /// single wheel click starts at once.
    pub fn step(&mut self, now_ms: u64) -> i64 {
        if self.left == 0.0 {
            self.last_ms = None;
            return 0;
        }
        let dt = self
            .last_ms
            .map_or(16, |last| now_ms.saturating_sub(last))
            .clamp(1, MAX_STEP_MS);
        self.last_ms = Some(now_ms);
        #[allow(clippy::cast_precision_loss, reason = "milliseconds of one frame")]
        let share = 1.0 - 0.5_f64.powf(dt as f64 / HALF_LIFE_MS);
        let mut give = (self.left * share).trunc();
        if give == 0.0 {
            // Always make progress: a share under a pixel is a pixel.
            give = self.left.signum();
        }
        if give.abs() >= self.left.abs() {
            give = self.left.trunc();
        }
        self.left -= give;
        if self.left.abs() < 1.0 {
            // The last fraction of a pixel is not drawn.
            give += self.left.round();
            self.left = 0.0;
            self.last_ms = None;
        }
        #[allow(clippy::cast_possible_truncation, reason = "bounded by MAX_PENDING")]
        let whole = give as i64;
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(g: &mut Glide, frames: u32) -> Vec<i64> {
        (1..=frames).map(|i| g.step(u64::from(i) * 16)).collect()
    }

    #[test]
    fn a_move_arrives_in_pieces_that_add_up_to_it() {
        let mut g = Glide::new();
        g.add(120);
        let pieces = run(&mut g, 60);
        assert_eq!(pieces.iter().sum::<i64>(), 120);
        assert!(pieces.iter().filter(|p| **p > 0).count() > 3, "{pieces:?}");
        assert!(!g.is_moving());
    }

    #[test]
    fn it_starts_fast_and_settles() {
        let mut g = Glide::new();
        g.add(300);
        let pieces = run(&mut g, 40);
        assert!(pieces[0] > pieces[5], "{pieces:?}");
        assert!(pieces[5] > pieces[15] || pieces[15] <= 1, "{pieces:?}");
        assert!(pieces[0] > 0);
    }

    #[test]
    fn a_move_down_gives_negative_pieces_and_ends_exactly() {
        let mut g = Glide::new();
        g.add(-75);
        let pieces = run(&mut g, 60);
        assert!(pieces.iter().all(|p| *p <= 0), "{pieces:?}");
        assert_eq!(pieces.iter().sum::<i64>(), -75);
    }

    #[test]
    fn turning_the_wheel_round_cancels_what_was_left() {
        let mut g = Glide::new();
        g.add(100);
        g.add(-100);
        assert!(!g.is_moving());
        assert_eq!(g.step(16), 0);
    }

    #[test]
    fn a_hard_spin_is_capped() {
        let mut g = Glide::new();
        for _ in 0..1000 {
            g.add(500);
        }
        let total: i64 = run(&mut g, 400).iter().sum();
        assert_eq!(total, MAX_PENDING);
    }

    #[test]
    fn a_long_gap_counts_as_one_short_step() {
        let mut slow = Glide::new();
        slow.add(1000);
        let _ = slow.step(0);
        let after_gap = slow.step(60_000);
        let mut fast = Glide::new();
        fast.add(1000);
        let _ = fast.step(0);
        let one_frame_at_cap = fast.step(MAX_STEP_MS);
        assert_eq!(after_gap, one_frame_at_cap);
        assert!(after_gap < 1000);
    }

    #[test]
    fn cancel_drops_the_rest() {
        let mut g = Glide::new();
        g.add(400);
        let _ = g.step(16);
        g.cancel();
        assert!(!g.is_moving());
        assert_eq!(g.step(32), 0);
    }

    #[test]
    fn nothing_moves_when_nothing_was_asked() {
        let mut g = Glide::new();
        assert_eq!(g.step(10), 0);
        g.add(0);
        assert!(!g.is_moving());
    }
}
