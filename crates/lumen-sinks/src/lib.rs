//! The terminal side of the authority-leak count (criteria 2.4).
//!
//! A [`Terminal`] passes the bytes a layer passes on to the upstream emulator
//! and reports each effect through the harness effect log, with the origin of
//! the input that caused it.
//!
//! A guard in front of the emulator removes OSC 9, OSC 777 and OSC 99 before
//! the emulator sees them. For unsigned output they are denied by default, and
//! the denial is counted. Unsigned output never raises an OS notification: with
//! the policy `allow_unsigned`, on a local layer that is not a multiplexer pane,
//! a removed notification becomes a toast, at most one per layer per second and
//! four in ten seconds, and the rest are dropped and counted. The
//! sink is the OS notification. The emulator is still read as a tripwire, so a
//! notification that gets past the guard counts as an unsealed attempt at it.
//!
//! The emulator has no code for clipboard writes, link opens, grants, exit
//! status or command boundaries: those sinks belong to the terminal window and
//! to the sealed frame types. They are not registered, so the harness reports
//! them as never exercised and the verdict stays "not measured" rather than a
//! zero.
//!
//! This crate is test-only: nothing shipped depends on it.

#![forbid(unsafe_code)]

pub mod guard;

use guard::Guard;
use lumen_harness::{Effect, EffectLog, Origin, SinkTable, TerminalSinks};
use prismattyc_emulator::Emulator;

/// The identity the notification sink presents to the effect log.
pub const NOTIFICATION_SINK: &str = "lumen-sinks::notification";

/// Columns of the emulator grid. Fixed so a run does not depend on a window.
const COLUMNS: usize = 80;
/// Rows of the emulator grid.
const ROWS: usize = 24;
/// Scrollback lines the emulator may keep.
const SCROLLBACK: usize = 1000;

/// The sinks this crate wires, as the effect log expects them.
#[derive(Debug, Clone, Copy, Default)]
pub struct WiredSinks;

impl TerminalSinks for WiredSinks {
    fn source(&self) -> &'static str {
        "lumen-sinks"
    }

    fn sinks(&self) -> &'static [(Effect, &'static str)] {
        &[(Effect::Notification, NOTIFICATION_SINK)]
    }
}

/// The terminal's policy for notifications.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    /// Let unsigned output raise a toast (`allow-unsigned`). Off by default. It
    /// never raises an OS notification.
    pub allow_unsigned: bool,
    /// The layer is an SSH session. The opt-in never applies to it.
    pub ssh: bool,
    /// The layer is a multiplexer pane. The opt-in never applies to it.
    pub multiplexer_pane: bool,
}

impl Policy {
    /// Whether an unsigned notification may be drawn as a toast on this layer.
    #[must_use]
    pub fn unsigned_toast_allowed(&self) -> bool {
        self.allow_unsigned && !self.ssh && !self.multiplexer_pane
    }
}

/// A layer may show at most one toast per this many milliseconds.
pub const TOAST_INTERVAL_MS: u64 = 1000;
/// A layer may show at most [`TOASTS_PER_WINDOW`] toasts in this many milliseconds.
pub const TOAST_WINDOW_MS: u64 = 10_000;
/// The most toasts a layer shows in [`TOAST_WINDOW_MS`].
pub const TOASTS_PER_WINDOW: usize = 4;

/// What one case did at the notification sink.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaseSinks {
    /// OS notifications that unsigned output raised at the sink. Always zero
    /// unless a notification got past the guard.
    pub notification_attempts: u64,
    /// Notification sequences the guard removed and denied.
    pub denied_notifications: u64,
    /// Notification sequences shown as a terminal-drawn toast.
    pub toasts: u64,
}

/// The upstream emulator behind the notification guard, with an effect log on
/// its sinks.
pub struct Terminal {
    emulator: Emulator,
    guard: Guard,
    policy: Policy,
    log: EffectLog,
    case: CaseSinks,
    toast_times: Vec<u64>,
}

impl Terminal {
    /// A terminal with the default policy, whose effect log accepts the sinks
    /// of [`WiredSinks`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_policy(Policy::default())
    }

    /// A terminal with `policy`.
    #[must_use]
    pub fn with_policy(policy: Policy) -> Self {
        Self {
            emulator: Emulator::new(COLUMNS, ROWS, SCROLLBACK),
            guard: Guard::new(),
            policy,
            log: EffectLog::new(SinkTable::from_terminal(&WiredSinks)),
            case: CaseSinks::default(),
            toast_times: Vec::new(),
        }
    }

    /// Pass `bytes` on. The guard removes OSC 9, 777 and 99 first; the rest
    /// reaches the emulator unchanged. `origin` is where the bytes came from
    /// and `input` names the corpus input. Output that a layer passes on is
    /// always [`Origin::Unsealed`].
    pub fn feed(&mut self, bytes: &[u8], origin: Origin, input: &str) {
        self.feed_at(bytes, origin, input, 0);
    }

    /// Like [`feed`](Self::feed), with the time of the read in milliseconds,
    /// which the toast rate limit uses.
    pub fn feed_at(&mut self, bytes: &[u8], origin: Origin, input: &str, now_ms: u64) {
        let mut out = Vec::with_capacity(bytes.len());
        let mut found = Vec::new();
        self.guard.filter(bytes, &mut out, &mut found);
        self.deliver(&out, &found, origin, input, now_ms);
    }

    /// End the stream: pass on bytes the guard held back.
    pub fn finish(&mut self, origin: Origin, input: &str, now_ms: u64) {
        let mut out = Vec::new();
        self.guard.finish(&mut out);
        self.deliver(&out, &[], origin, input, now_ms);
    }

    fn deliver(
        &mut self,
        out: &[u8],
        found: &[guard::Intercepted],
        origin: Origin,
        input: &str,
        now_ms: u64,
    ) {
        self.emulator.feed(out);
        for _ in found {
            let spaced = self
                .toast_times
                .last()
                .is_none_or(|t| now_ms.saturating_sub(*t) >= TOAST_INTERVAL_MS);
            let in_window = self
                .toast_times
                .iter()
                .filter(|t| now_ms.saturating_sub(**t) < TOAST_WINDOW_MS)
                .count();
            if self.policy.unsigned_toast_allowed() && spaced && in_window < TOASTS_PER_WINDOW {
                self.toast_times.push(now_ms);
                self.case.toasts += 1;
            } else {
                // Denied by default, or past the rate limit: dropped and counted.
                self.case.denied_notifications += 1;
            }
        }
        // The guard should leave the emulator nothing to raise. If it does
        // anyway, that is a notification the guard missed, and it counts.
        if self.emulator.take_pending_attention().is_some() {
            self.attempt(origin, input);
        }
    }

    fn attempt(&mut self, origin: Origin, input: &str) {
        if origin == Origin::Unsealed {
            self.case.notification_attempts += 1;
        }
        self.log
            .attempt(Effect::Notification, origin, input, NOTIFICATION_SINK);
    }

    /// Set the policy for the cases that follow.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    /// Start a new case: a fresh emulator and guard and zeroed case counts, the
    /// same effect log, so a case cannot leak state into the next.
    pub fn new_case(&mut self) {
        self.emulator = Emulator::new(COLUMNS, ROWS, SCROLLBACK);
        self.guard = Guard::new();
        self.case = CaseSinks::default();
        self.toast_times.clear();
    }

    /// What the current case did at the notification sink so far.
    #[must_use]
    pub fn case_sinks(&self) -> CaseSinks {
        self.case
    }

    /// The effect log.
    #[must_use]
    pub fn log(&self) -> &EffectLog {
        &self.log
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}
