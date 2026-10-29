//! The terminal side of the authority-leak count (criteria 2.4).
//!
//! A [`Terminal`] feeds the bytes a layer passes on to the upstream emulator,
//! unchanged, and reports each effect the emulator raises through the harness
//! effect log, with the origin of the input that caused it.
//!
//! Only effects that the upstream emulator actually raises are wired here, and
//! each is read from the emulator itself, so a count is a measurement of that
//! code. Today that is the notification sink (the OSC 9, OSC 777 and OSC 99
//! notifications the emulator queues). The emulator has no code for clipboard
//! writes, link opens, grants, exit status or command boundaries: those sinks
//! belong to the terminal window and the sealed command types, neither of
//! which exists in this tree. They are not registered, so the harness reports
//! them as never exercised and the verdict stays "not measured" rather than a
//! zero.
//!
//! This crate is test-only: nothing shipped depends on it.

#![forbid(unsafe_code)]

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

/// The upstream emulator with an effect log on its sinks.
pub struct Terminal {
    emulator: Emulator,
    log: EffectLog,
}

impl Terminal {
    /// A terminal whose effect log accepts the sinks of [`WiredSinks`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            emulator: Emulator::new(COLUMNS, ROWS, SCROLLBACK),
            log: EffectLog::new(SinkTable::from_terminal(&WiredSinks)),
        }
    }

    /// Feed `bytes` to the emulator. `origin` is where they came from and
    /// `input` names the corpus input. Output that a layer passes on is always
    /// [`Origin::Unsealed`].
    pub fn feed(&mut self, bytes: &[u8], origin: Origin, input: &str) {
        self.emulator.feed(bytes);
        if self.emulator.take_pending_attention().is_some() {
            self.log
                .attempt(Effect::Notification, origin, input, NOTIFICATION_SINK);
        }
    }

    /// Start a new case: a fresh emulator, the same effect log, so a case
    /// cannot leak state into the next.
    pub fn new_case(&mut self) {
        self.emulator = Emulator::new(COLUMNS, ROWS, SCROLLBACK);
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
