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
//! Sealed frames are routed to the other five sinks by [`Terminal::sealed`]:
//! command start, prompt and command end reach the command-boundary and
//! exit-status sinks, a grant reaches the grant sink, a clipboard write reaches
//! the clipboard sink after the terminal's own confirmation, and a notification
//! is a toast or an OS notification. A link opens only on a confirmed user
//! action, [`Terminal::user_opens_link`].
//!
//! Two user actions are simulated, because the harness has no user: the
//! clipboard confirmation ([`Desk::confirm_clipboard`]) and the link
//! confirmation. [`Terminal::simulated`] lists the ones a run used, and the
//! report must print them.
//!
//! This crate is test-only: nothing shipped depends on it.

#![forbid(unsafe_code)]

pub mod guard;
pub mod sealed;

use guard::Guard;
use lumen_harness::{Effect, EffectLog, Origin, SinkTable, TerminalSinks};
use prismattyc_emulator::Emulator;

/// Columns of the emulator grid. Fixed so a run does not depend on a window.
const COLUMNS: usize = 80;
/// Rows of the emulator grid.
const ROWS: usize = 24;
/// Scrollback lines the emulator may keep.
const SCROLLBACK: usize = 1000;

/// The identity each sink presents to the effect log.
fn sink_id(effect: Effect) -> &'static str {
    match effect {
        Effect::ClipboardWrite => "lumen-sinks::clipboard_write",
        Effect::Notification => "lumen-sinks::notification",
        Effect::LinkOpen => "lumen-sinks::link_open",
        Effect::Grant => "lumen-sinks::grant",
        Effect::ExitStatus => "lumen-sinks::exit_status",
        Effect::BoundaryChange => "lumen-sinks::boundary_change",
    }
}

/// The identity the notification sink presents to the effect log.
pub const NOTIFICATION_SINK: &str = "lumen-sinks::notification";

/// The sinks this crate wires, as the effect log expects them.
#[derive(Debug, Clone, Copy, Default)]
pub struct WiredSinks;

impl TerminalSinks for WiredSinks {
    fn source(&self) -> &'static str {
        "lumen-sinks"
    }

    fn sinks(&self) -> &'static [(Effect, &'static str)] {
        &[
            (Effect::ClipboardWrite, "lumen-sinks::clipboard_write"),
            (Effect::Notification, NOTIFICATION_SINK),
            (Effect::LinkOpen, "lumen-sinks::link_open"),
            (Effect::Grant, "lumen-sinks::grant"),
            (Effect::ExitStatus, "lumen-sinks::exit_status"),
            (Effect::BoundaryChange, "lumen-sinks::boundary_change"),
        ]
    }
}

/// The terminal's policy file keys for notifications.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    /// `notifications.allow-unsigned`: let unsigned grid output draw a toast.
    /// Off by default. It never raises an OS notification.
    pub allow_unsigned: bool,
    /// `notifications.os`: allow OS notifications from sealed frames. Off by default.
    pub os_notifications: bool,
    /// `notifications.os-text`: let an OS notification carry the program's text.
    /// Off by default.
    pub os_text: bool,
}

/// What kind of layer the output is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layer {
    /// The layer is an SSH session. The opt-in never applies to it.
    pub ssh: bool,
    /// The layer is a multiplexer pane. The opt-in never applies to it.
    pub multiplexer_pane: bool,
}

/// The window and the user, which the harness can only simulate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Desk {
    /// The terminal window is unfocused.
    pub unfocused: bool,
    /// The user has muted the pane.
    pub muted: bool,
    /// The user confirms a clipboard write when asked. Simulated.
    pub confirm_clipboard: bool,
}

impl Policy {
    /// Whether an unsigned notification may be drawn as a toast on `layer`.
    #[must_use]
    pub fn unsigned_toast_allowed(&self, layer: Layer) -> bool {
        self.allow_unsigned && !layer.ssh && !layer.multiplexer_pane
    }
}

/// At least this many milliseconds between two notifications from a layer.
pub const NOTIFICATION_INTERVAL_MS: u64 = 1000;
/// A layer may show at most [`TOASTS_PER_WINDOW`] toasts in this many milliseconds.
pub const TOAST_WINDOW_MS: u64 = 10_000;
/// The most toasts a layer shows in [`TOAST_WINDOW_MS`].
pub const TOASTS_PER_WINDOW: usize = 4;
/// The most bytes of program text in a notification.
pub const MAX_NOTIFICATION_BYTES: usize = 200;
/// The most terminal cells of program text in a notification.
pub const MAX_NOTIFICATION_CELLS: usize = 60;

/// What one case did at the sinks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaseSinks {
    /// OS notifications that reached the sink from unsigned output. Always zero
    /// unless a notification got past the guard.
    pub notification_attempts: u64,
    /// Notification sequences the guard removed and denied, or dropped over a limit.
    pub denied_notifications: u64,
    /// Notifications shown as a terminal-drawn toast.
    pub toasts: u64,
    /// OS notifications raised from sealed frames.
    pub os_notifications: u64,
    /// Sealed frames refused: unknown type, malformed, out of sequence, or
    /// without the capability.
    pub refused: u64,
    /// Attempts at each sink in this case, from any origin, by effect name.
    pub effects: std::collections::BTreeMap<&'static str, u64>,
    /// The text of each OS notification raised, in order.
    pub os_texts: Vec<String>,
}

/// The upstream emulator behind the notification guard, with an effect log on
/// its sinks.
pub struct Terminal {
    emulator: Emulator,
    guard: Guard,
    guard_enabled: bool,
    policy: Policy,
    layer: Layer,
    desk: Desk,
    log: EffectLog,
    case: CaseSinks,
    accepted_times: Vec<u64>,
    toast_times: Vec<u64>,
    orphan: bool,
    command: sealed::CommandState,
    granted: [bool; 9],
    simulated: Vec<&'static str>,
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
            guard_enabled: true,
            policy,
            layer: Layer::default(),
            desk: Desk::default(),
            log: EffectLog::new(SinkTable::from_terminal(&WiredSinks)),
            case: CaseSinks::default(),
            accepted_times: Vec::new(),
            toast_times: Vec::new(),
            orphan: false,
            command: sealed::CommandState::default(),
            granted: [false; 9],
            simulated: Vec::new(),
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
        if self.guard_enabled {
            self.guard.filter(bytes, &mut out, &mut found);
        } else {
            out.extend_from_slice(bytes);
        }
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
            let allowed = self.policy.unsigned_toast_allowed(self.layer) && !self.orphan;
            if allowed && self.toast_ok(now_ms) {
                self.accept(now_ms, true);
                self.case.toasts += 1;
            } else {
                // Denied by default, or past a rate limit: dropped and counted.
                self.case.denied_notifications += 1;
            }
        }
        // The guard should leave the emulator nothing to raise. If it does
        // anyway, that is a notification the guard missed, and it counts.
        if self.emulator.take_pending_attention().is_some() {
            if origin == Origin::Unsealed {
                self.case.notification_attempts += 1;
            }
            self.attempt(Effect::Notification, origin, input);
        }
    }

    /// Whether a notification may be accepted from the layer now: at least
    /// [`NOTIFICATION_INTERVAL_MS`] after the last one.
    fn spaced(&self, now_ms: u64) -> bool {
        self.accepted_times
            .last()
            .is_none_or(|t| now_ms.saturating_sub(*t) >= NOTIFICATION_INTERVAL_MS)
    }

    /// Whether a toast may be drawn now: spaced, and under the window limit.
    fn toast_ok(&self, now_ms: u64) -> bool {
        let in_window = self
            .toast_times
            .iter()
            .filter(|t| now_ms.saturating_sub(**t) < TOAST_WINDOW_MS)
            .count();
        self.spaced(now_ms) && in_window < TOASTS_PER_WINDOW
    }

    /// Record an accepted notification.
    fn accept(&mut self, now_ms: u64, toast: bool) {
        self.accepted_times.push(now_ms);
        if toast {
            self.toast_times.push(now_ms);
        }
    }

    /// Record an attempt at the sink for `effect`.
    fn attempt(&mut self, effect: Effect, origin: Origin, input: &str) {
        *self.case.effects.entry(effect.name()).or_insert(0) += 1;
        self.log.attempt(effect, origin, input, sink_id(effect));
    }

    /// Turn the guard off. This is a negative control for tests: with it off, a
    /// notification in plain output reaches the emulator, and the count must
    /// not be zero. A run that reports a result never calls it.
    pub fn disable_guard_for_negative_control(&mut self) {
        self.guard_enabled = false;
    }

    /// Say whether the output is orphan output (no command container is open).
    /// The opt-in covers unsigned grid output only, so it never applies to it.
    pub fn set_orphan(&mut self, orphan: bool) {
        self.orphan = orphan;
    }

    /// Set the policy for the cases that follow.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    /// Set what kind of layer the cases that follow run on.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Set the window and simulated user state for the cases that follow.
    pub fn set_desk(&mut self, desk: Desk) {
        self.desk = desk;
    }

    /// Start a new case: a fresh emulator, guard, layer state and case counts,
    /// the same effect log, so a case cannot leak state into the next.
    pub fn new_case(&mut self) {
        self.emulator = Emulator::new(COLUMNS, ROWS, SCROLLBACK);
        self.guard = Guard::new();
        self.case = CaseSinks::default();
        self.accepted_times.clear();
        self.toast_times.clear();
        self.command = sealed::CommandState::default();
        self.granted = [false; 9];
    }

    /// What the current case did at the sinks so far.
    #[must_use]
    pub fn case_sinks(&self) -> &CaseSinks {
        &self.case
    }

    /// The simulated user actions this terminal used, for the report.
    #[must_use]
    pub fn simulated(&self) -> &[&'static str] {
        &self.simulated
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
