//! Routing of verified sealed frames to the terminal's sinks.

use crate::{MAX_NOTIFICATION_BYTES, MAX_NOTIFICATION_CELLS, Terminal};
use lumen_harness::{Effect, Origin};
use lumen_sealed::types::{
    CAP_CLIPBOARD_WRITE, CAP_NOTIFICATIONS, Frame, Malformed, capability_of, parse,
};

/// The fixed label of the layer in a notification: its position in the
/// terminal's layout. A program cannot choose it.
const PANE_LABEL: &str = "pane 1";

/// Command ids in a layer: the next expected start and the open command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandState {
    /// Ids handed out so far.
    last_id: u32,
    /// The command that is open, if any.
    open: Option<u32>,
}

/// Why the terminal refused a sealed frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The type is not in the table.
    UnknownKind,
    /// The payload did not parse.
    Malformed(Malformed),
    /// A command id out of sequence, or a command end with no matching start.
    OutOfSequence,
    /// The layer does not hold the capability the frame needs.
    NotGranted,
    /// The policy does not allow the capability to be granted.
    PolicyDenied,
}

/// What the terminal did with a sealed frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The frame took effect, or was parsed and has none by design.
    Applied,
    /// The frame was refused.
    Refused(Refusal),
    /// A clipboard write waited for a confirmation that did not come.
    Unconfirmed,
}

/// Remove what a program must not put into a notification: control and
/// formatting characters. The result is one line.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    u32::from(*c),
                    0x200b..=0x200f | 0x202a..=0x202e | 0x2060 | 0x2066..=0x2069 | 0x061c | 0xfeff
                )
        })
        .collect()
}

/// Cap `text` at the byte and cell limits, ending with a visible marker if it
/// was cut.
fn cap(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if out.len() + c.len_utf8() > MAX_NOTIFICATION_BYTES
            || out.chars().count() >= MAX_NOTIFICATION_CELLS
        {
            out.push('\u{2026}');
            return out;
        }
        out.push(c);
    }
    out
}

impl Terminal {
    /// Route the sealed frame of type `kind` with payload `body`, which the
    /// layer verified and opened. `now_ms` is the time of the read and `input`
    /// names the corpus input.
    pub fn sealed(&mut self, kind: u8, body: &[u8], now_ms: u64, input: &str) -> Outcome {
        if capability_of(kind).is_none() {
            return self.refuse(Refusal::UnknownKind);
        }
        match parse(kind, body) {
            Err(e) => self.refuse(Refusal::Malformed(e)),
            Ok(frame) => self.apply(frame, now_ms, input),
        }
    }

    fn refuse(&mut self, why: Refusal) -> Outcome {
        self.case.refused += 1;
        Outcome::Refused(why)
    }

    fn holds(&self, capability: u8) -> bool {
        self.granted
            .get(usize::from(capability))
            .copied()
            .unwrap_or(false)
    }

    fn apply(&mut self, frame: Frame, now_ms: u64, input: &str) -> Outcome {
        match frame {
            Frame::Layout(_) | Frame::PaneContainer(_) | Frame::Marker { .. } => Outcome::Applied,
            Frame::CommandStart { id } => {
                if id != self.command.last_id.wrapping_add(1) {
                    return self.refuse(Refusal::OutOfSequence);
                }
                // A start while a command is open ends that one as unknown.
                self.command.last_id = id;
                self.command.open = Some(id);
                self.attempt(Effect::BoundaryChange, Origin::Sealed, input);
                Outcome::Applied
            }
            Frame::Prompt => {
                self.attempt(Effect::BoundaryChange, Origin::Sealed, input);
                Outcome::Applied
            }
            Frame::CommandEnd { id, .. } => {
                if self.command.open != Some(id) {
                    return self.refuse(Refusal::OutOfSequence);
                }
                self.command.open = None;
                self.attempt(Effect::ExitStatus, Origin::Sealed, input);
                self.attempt(Effect::BoundaryChange, Origin::Sealed, input);
                Outcome::Applied
            }
            Frame::Grant { grant, capability } => {
                if grant && !self.policy.allows_grant(capability) {
                    return self.refuse(Refusal::PolicyDenied);
                }
                if let Some(slot) = self.granted.get_mut(usize::from(capability)) {
                    *slot = grant;
                }
                self.attempt(Effect::Grant, Origin::Sealed, input);
                Outcome::Applied
            }
            Frame::ClipboardWrite(_) => {
                if !self.holds(CAP_CLIPBOARD_WRITE) {
                    return self.refuse(Refusal::NotGranted);
                }
                if !self.desk.confirm_clipboard {
                    return Outcome::Unconfirmed;
                }
                self.simulate("clipboard_confirmation");
                self.attempt(Effect::ClipboardWrite, Origin::Sealed, input);
                Outcome::Applied
            }
            Frame::Notification(text) => {
                if !self.holds(CAP_NOTIFICATIONS) {
                    return self.refuse(Refusal::NotGranted);
                }
                self.sealed_notification(&text, now_ms, input);
                Outcome::Applied
            }
        }
    }

    /// A sealed notification: an OS notification if every condition holds, else
    /// a toast if the toast limits allow, else dropped and counted.
    fn sealed_notification(&mut self, text: &str, now_ms: u64, input: &str) {
        if !self.spaced(now_ms) {
            self.case.denied_notifications += 1;
            return;
        }
        let os_allowed = self.policy.os_notifications
            && self.desk.unfocused
            && !self.desk.muted
            && !self.layer.ssh
            && !self.layer.multiplexer_pane;
        if os_allowed {
            self.accept(now_ms, false);
            let mut shown = format!("Program in {PANE_LABEL} says:");
            if self.policy.os_text {
                shown.push(' ');
                shown.push_str(&cap(&clean(text)));
            }
            self.case.os_notifications += 1;
            self.case.os_texts.push(shown);
            self.attempt(Effect::Notification, Origin::Sealed, input);
        } else if self.toast_ok(now_ms) {
            self.accept(now_ms, true);
            self.case.toasts += 1;
        } else {
            self.case.denied_notifications += 1;
        }
    }

    fn simulate(&mut self, what: &'static str) {
        if !self.simulated.contains(&what) {
            self.simulated.push(what);
        }
    }

    /// The user activates a link whose full target the terminal showed.
    /// Nothing reaches the sink unless the user confirmed. The confirmation is
    /// simulated by the harness and is listed in [`Terminal::simulated`].
    pub fn user_opens_link(&mut self, confirmed: bool, input: &str) {
        if confirmed {
            self.simulate("link_confirmation");
            self.attempt(Effect::LinkOpen, Origin::User, input);
        }
    }
}
