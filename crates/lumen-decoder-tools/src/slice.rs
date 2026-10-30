//! Sizing the next slice of a format so that the CPU-hour cap holds.
//!
//! A fuzz slice costs a fixed startup (loading the corpus, starting the
//! instrumented target: 3 CPU-seconds measured on a laptop and more than 8 on a
//! small cloud machine, so 30 are allowed; against hours of CPU that is nothing) plus a rate per worker-second (about 1
//! CPU-second, a little more when the fuzzer's parent process is busy). CPU time
//! is only known after the slice, so the last slice is sized from what is left of
//! the cap, assuming the worst of both:
//!
//! - the startup is at most [`STARTUP_CPU_SECONDS`];
//! - the rate is at most 1.25 times the highest rate seen on the earlier slices,
//!   where a slice's rate is its whole CPU time over workers x wall time, startup
//!   included, so it can only overstate (the margin [`MARGIN`] is 1 / 1.25).
//!
//! **Under those two assumptions the counted hours never pass the cap.** When one
//! breaks, the cap is passed by the excess, and nothing else bounds it; the
//! simulation test in `tests/slice.rs` shows both. Until a rate has been seen,
//! [`DEFAULT_RATE`] is used and the first slice takes at most [`FIRST_CUT_SHARE`]
//! of what is left, so a rate is learned before the last slice.

/// Startup CPU allowed per slice, in seconds.
pub const STARTUP_CPU_SECONDS: f64 = 30.0;
/// CPU seconds per worker-second assumed until a slice has been seen.
pub const DEFAULT_RATE: f64 = 1.2;
/// Fraction of the planned time actually used (1 / 1.25).
pub const MARGIN: f64 = 0.8;
/// Largest share of what is left that the first slice, before any rate is known, may take.
pub const FIRST_CUT_SHARE: f64 = 0.5;
/// A slice shorter than this does little more than load its corpus.
pub const MIN_SLICE_SECONDS: f64 = 5.0;

/// Seconds the next slice of a format may run; 0 means the cap or the wall limit
/// is reached.
///
/// `slice` is the normal slice length, `cap_hours` and `done_hours` are in CPU
/// hours, `learned_rate` is the highest rate seen so far (startup included), and
/// `wall_left` is the seconds left before the wall-clock limit, if there is one.
#[must_use]
pub fn next_slice_seconds(
    slice: f64,
    workers: u32,
    cap_hours: f64,
    done_hours: f64,
    learned_rate: Option<f64>,
    wall_left: Option<f64>,
) -> u64 {
    let left = ((cap_hours - done_hours) * 3600.0 - STARTUP_CPU_SECONDS).max(0.0);
    let rate = learned_rate.unwrap_or(DEFAULT_RATE);
    let mut planned = left / (f64::from(workers) * rate) * MARGIN;
    if learned_rate.is_none() {
        // Nothing is known about the rate yet: the first slice may take at most
        // half of what is left, whether or not it is cut, so that even a rate
        // three times the default cannot pass the cap before one has been seen.
        planned *= FIRST_CUT_SHARE;
    }
    let mut seconds = slice.min(planned);
    if let Some(wall) = wall_left {
        seconds = seconds.min(wall);
    }
    if seconds < MIN_SLICE_SECONDS {
        0
    } else {
        // Rounded down: with many workers one extra second costs as much as the
        // margin of the shortest slice allowed.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 5 <= seconds <= slice
        let whole = seconds.floor() as u64;
        whole
    }
}

/// The rate to remember after a slice: the highest of the previous one and this
/// slice's whole CPU time per worker-second (startup included, which can only
/// overstate it), never below 0.5.
#[must_use]
pub fn learn_rate(previous: Option<f64>, cpu_seconds: f64, workers: u32, wall_seconds: f64) -> f64 {
    let this = (cpu_seconds / (f64::from(workers) * wall_seconds.max(1.0))).max(0.5);
    previous.map_or(this, |p| p.max(this))
}
