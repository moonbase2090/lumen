//! Notifications: terminal toasts and OS notifications.
//!
//! This is the logic of the Notifications section of `trust.md`, headless: it
//! decides, from the settings, the layer, the window and the clock, whether a
//! notification is dropped, drawn as a toast, or passed to the operating
//! system, and it composes the text. Nothing here draws or calls the OS: the
//! toasts are for the renderer, and an [`OsSink`] receives an OS notification.
//!
//! The settings default to off. Unsigned output never produces an OS
//! notification. An OS notification comes only from a sealed `0x22` frame under
//! the `notifications` capability, with `os_notifications` set, an unfocused
//! terminal, a local layer, an unmuted pane and the OS budget unspent.
//!
//! The rate numbers are the section's provisional proposals.

use crate::document::Event;
use lumen_sealed::types::{CAP_NOTIFICATIONS, Frame, KIND_GRANT, KIND_NOTIFICATION, parse};
use prismattyc_core::grapheme_display_width;
use std::collections::{BTreeMap, BTreeSet};
use unicode_general_category::{GeneralCategory as Gc, get_general_category};

/// A layer's id. Ids are never reused.
pub type LayerId = u32;

/// At least this long between two notifications accepted from a layer.
pub const SPACING_MS: u64 = 1000;
/// A layer shows at most [`TOASTS_PER_WINDOW`] toasts in this window.
pub const TOAST_WINDOW_MS: u64 = 10_000;
/// Toasts a layer shows in [`TOAST_WINDOW_MS`].
pub const TOASTS_PER_WINDOW: usize = 4;
/// Toasts a layer may have shown at once.
pub const LAYER_TOAST_CAP: usize = 8;
/// Toasts shown at once across all layers.
pub const TOAST_CAP: usize = 16;
/// How long a toast stays.
pub const TOAST_LIFETIME_MS: u64 = 6000;
/// The window of the OS budget.
pub const OS_WINDOW_MS: u64 = 60_000;
/// OS notifications per layer in [`OS_WINDOW_MS`].
pub const OS_PER_LAYER: usize = 3;
/// OS notifications across all layers in [`OS_WINDOW_MS`].
pub const OS_GLOBAL: usize = 8;
/// With the OS budget spent, a layer is toasts only until the user acts on it
/// or this long passes with no notification from it.
pub const OS_RECOVERY_MS: u64 = 600_000;
/// The most bytes of program text.
pub const MAX_TEXT_BYTES: usize = 200;
/// The most terminal cells of program text.
pub const MAX_TEXT_CELLS: usize = 60;
/// Ends a truncated text.
pub const MARKER: char = '\u{2026}';

/// The notification settings, from the user's policy file. All default to off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    /// `allow_unsigned`: unsigned grid output on a local layer may draw a toast.
    pub allow_unsigned: bool,
    /// `os_notifications`: a sealed notification may be an OS notification.
    pub os_notifications: bool,
    /// `os_text`: an OS notification may carry the program's text.
    pub os_text: bool,
}

/// Where a layer runs, as far as the terminal can tell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Place {
    /// The foreground process tree is shown to be local.
    Local,
    /// An SSH session.
    Ssh,
    /// A multiplexer pane.
    MultiplexerPane,
    /// Possibly remote.
    PossiblyRemote,
    /// The terminal cannot tell. A layer it cannot tell about is not local.
    #[default]
    Unknown,
}

impl Place {
    /// A layer is local only when it is shown local.
    #[must_use]
    pub fn is_local(self) -> bool {
        self == Place::Local
    }
}

/// The window and the user.
///
/// The default is a focused terminal that is not muted: until the host reports
/// otherwise the terminal assumes it has focus, so a notification is a toast
/// and never an OS notification. Only a host that says the terminal is
/// unfocused can open the OS path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desk {
    /// The terminal has focus.
    pub focused: bool,
    /// The user muted the terminal's notifications.
    pub muted: bool,
}

impl Default for Desk {
    fn default() -> Self {
        Self {
            focused: true,
            muted: false,
        }
    }
}

/// Where a notification came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// An OSC sequence in unsigned grid output.
    UnsignedGrid,
    /// A sealed `0x22` frame that held the `notifications` capability.
    Sealed,
}

/// Why a notification was dropped. Every drop adds one to the chrome count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// Unsigned output without `allow_unsigned`, or on a layer that is not local.
    Denied,
    /// The pane, the layer or the terminal is muted.
    Muted,
    /// Under a second after the last notification from the layer.
    Spacing,
    /// The layer already showed its toasts for this window.
    ToastLimit,
    /// All toasts are shown and the layer has none of its own to replace.
    NoRoom,
    /// The layer is unknown or closed.
    NoLayer,
}

/// A toast the terminal draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// The layer it belongs to.
    pub layer: LayerId,
    /// The composed text: prefix, label and the program's cleaned text.
    pub text: String,
    /// It came from unsigned output, so it carries the unverified-output label.
    pub unsigned: bool,
    /// When it was shown.
    pub shown_ms: u64,
}

/// An OS notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsNotification {
    /// The layer it came from.
    pub layer: LayerId,
    /// The composed text, escaped for the notification API.
    pub text: String,
    /// The token the OS callback carries: made by the terminal, random in a
    /// real build, single use, and bound to this layer.
    pub token: u64,
}

/// What happened to a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A toast was shown.
    Toast,
    /// An OS notification was passed on.
    Os(OsNotification),
    /// It was dropped and counted.
    Dropped(Dropped),
}

/// Takes an OS notification. The real one calls the operating system.
pub trait OsSink {
    /// Pass the notification on.
    fn send(&mut self, notification: &OsNotification);
}

/// A sink that keeps what it was given, for tests.
#[derive(Debug, Default)]
pub struct Recorder {
    /// Everything sent, in order.
    pub sent: Vec<OsNotification>,
}

impl OsSink for Recorder {
    fn send(&mut self, notification: &OsNotification) {
        self.sent.push(notification.clone());
    }
}

#[derive(Debug)]
struct LayerState {
    label: String,
    place: Place,
    muted: bool,
    granted: bool,
    accepted: Vec<u64>,
    toast_times: Vec<u64>,
    os_times: Vec<u64>,
    os_blocked: bool,
    last_ms: Option<u64>,
}

/// All the notification state of a terminal.
#[derive(Debug, Default)]
pub struct Notifier {
    settings: Settings,
    desk: Desk,
    layers: BTreeMap<LayerId, LayerState>,
    used: BTreeSet<LayerId>,
    global_os: Vec<u64>,
    shown: Vec<Toast>,
    dropped: u64,
    next_token: u64,
}

fn within(times: &[u64], now_ms: u64, window: u64) -> usize {
    times
        .iter()
        .filter(|t| now_ms.saturating_sub(**t) < window)
        .count()
}

impl Notifier {
    /// A notifier with `settings`.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    /// Set what the window and the user are doing.
    pub fn set_desk(&mut self, desk: Desk) {
        self.desk = desk;
    }

    /// Add a layer. `label` is the pane label the terminal made from the
    /// layer's position and its opaque id; it is never program text. Returns
    /// false if the id was used before: ids are never reused.
    pub fn add_layer(&mut self, id: LayerId, label: &str, place: Place) -> bool {
        if !self.used.insert(id) {
            return false;
        }
        self.layers.insert(
            id,
            LayerState {
                label: label.to_owned(),
                place,
                muted: false,
                granted: false,
                accepted: Vec::new(),
                toast_times: Vec::new(),
                os_times: Vec::new(),
                os_blocked: false,
                last_ms: None,
            },
        );
        true
    }

    /// A layer closed: its queued and shown toasts go with it.
    pub fn close_layer(&mut self, id: LayerId) {
        self.layers.remove(&id);
        self.shown.retain(|t| t.layer != id);
    }

    /// The user mutes or unmutes a layer. Muting silences its toasts too.
    /// Unmuting is the user acting on the layer, which ends a spent OS budget.
    pub fn set_muted(&mut self, id: LayerId, muted: bool) {
        if let Some(l) = self.layers.get_mut(&id) {
            l.muted = muted;
            if !muted {
                l.os_blocked = false;
            }
        }
    }

    /// The user opened the layer.
    pub fn user_opened(&mut self, id: LayerId) {
        if let Some(l) = self.layers.get_mut(&id) {
            l.os_blocked = false;
        }
    }

    /// The user revokes the `notifications` capability for a layer.
    pub fn revoke(&mut self, id: LayerId) {
        if let Some(l) = self.layers.get_mut(&id) {
            l.granted = false;
        }
    }

    /// Whether the layer holds the `notifications` capability.
    #[must_use]
    pub fn granted(&self, id: LayerId) -> bool {
        self.layers.get(&id).is_some_and(|l| l.granted)
    }

    /// The toasts now shown, oldest first.
    #[must_use]
    pub fn shown(&self) -> &[Toast] {
        &self.shown
    }

    /// The chrome count of dropped items.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Remove the toasts whose time is up.
    pub fn tick(&mut self, now_ms: u64) {
        self.shown
            .retain(|t| now_ms.saturating_sub(t.shown_ms) < TOAST_LIFETIME_MS);
    }

    /// The user dismissed the toast at `index` of [`shown`](Self::shown).
    pub fn dismiss(&mut self, index: usize) {
        if index < self.shown.len() {
            self.shown.remove(index);
        }
    }

    fn drop_with(&mut self, why: Dropped) -> Outcome {
        self.dropped += 1;
        Outcome::Dropped(why)
    }

    /// A notification arrives from `source` on layer `id` at `now_ms`, with the
    /// program's `text` if it has any.
    pub fn notify(&mut self, now_ms: u64, id: LayerId, source: Source, text: &str) -> Outcome {
        self.tick(now_ms);
        let Some(layer) = self.layers.get(&id) else {
            return self.drop_with(Dropped::NoLayer);
        };
        let unsigned = source == Source::UnsignedGrid;
        if unsigned && !(self.settings.allow_unsigned && layer.place.is_local()) {
            return self.drop_with(Dropped::Denied);
        }
        if layer.muted || self.desk.muted {
            return self.drop_with(Dropped::Muted);
        }
        if layer
            .last_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < SPACING_MS)
        {
            return self.drop_with(Dropped::Spacing);
        }
        let label = layer.label.clone();
        let outcome = if !unsigned && self.os_conditions(now_ms, id) {
            let token = self.next_token;
            self.next_token += 1;
            let body = if self.settings.os_text {
                Some(text)
            } else {
                None
            };
            let note = OsNotification {
                layer: id,
                text: escape_markup(&compose(&label, body)),
                token,
            };
            self.global_os.push(now_ms);
            if let Some(l) = self.layers.get_mut(&id) {
                l.os_times.push(now_ms);
            }
            Outcome::Os(note)
        } else {
            match self.show_toast(now_ms, id, &compose(&label, Some(text)), unsigned) {
                Ok(()) => Outcome::Toast,
                Err(why) => return self.drop_with(why),
            }
        };
        if let Some(l) = self.layers.get_mut(&id) {
            l.accepted.push(now_ms);
            l.last_ms = Some(now_ms);
        }
        outcome
    }

    /// The OS path's conditions, evaluated now. A layer whose own budget is
    /// spent is blocked until the user acts on it or ten quiet minutes pass.
    fn os_conditions(&mut self, now_ms: u64, id: LayerId) -> bool {
        let global = within(&self.global_os, now_ms, OS_WINDOW_MS);
        let Some(layer) = self.layers.get_mut(&id) else {
            return false;
        };
        if layer.os_blocked
            && layer
                .last_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= OS_RECOVERY_MS)
        {
            layer.os_blocked = false;
        }
        if !(self.settings.os_notifications && layer.place.is_local() && !self.desk.focused) {
            return false;
        }
        if layer.os_blocked {
            return false;
        }
        if within(&layer.os_times, now_ms, OS_WINDOW_MS) >= OS_PER_LAYER {
            layer.os_blocked = true;
            return false;
        }
        global < OS_GLOBAL
    }

    fn show_toast(
        &mut self,
        now_ms: u64,
        id: LayerId,
        text: &str,
        unsigned: bool,
    ) -> Result<(), Dropped> {
        let layer = self.layers.get_mut(&id).ok_or(Dropped::NoLayer)?;
        if within(&layer.toast_times, now_ms, TOAST_WINDOW_MS) >= TOASTS_PER_WINDOW {
            return Err(Dropped::ToastLimit);
        }
        let own = self.shown.iter().filter(|t| t.layer == id).count();
        let full = self.shown.len() >= TOAST_CAP || own >= LAYER_TOAST_CAP;
        if full {
            // Replace this layer's own oldest toast, never another layer's.
            let Some(at) = self.shown.iter().position(|t| t.layer == id) else {
                return Err(Dropped::NoRoom);
            };
            self.shown.remove(at);
        }
        layer.toast_times.push(now_ms);
        self.shown.push(Toast {
            layer: id,
            text: text.to_owned(),
            unsigned,
            shown_ms: now_ms,
        });
        Ok(())
    }

    /// Route the events a document raised on layer `id`. A sealed `0x20` grant
    /// or revoke of the `notifications` capability changes what the layer may
    /// do; a sealed `0x22` frame is a notification if the layer holds the
    /// capability, and an unsigned sequence is a notification from unsigned
    /// output. OS notifications go to `sink`. Anything else is not for this
    /// module and is ignored.
    pub fn route(
        &mut self,
        now_ms: u64,
        id: LayerId,
        events: &[Event],
        sink: &mut dyn OsSink,
    ) -> Vec<Outcome> {
        let mut out = Vec::new();
        for event in events {
            match event {
                Event::Unsigned { .. } => {
                    out.push(self.notify(now_ms, id, Source::UnsignedGrid, ""));
                }
                Event::Sealed { kind, body } if *kind == KIND_GRANT => {
                    if let Ok(Frame::Grant { grant, capability }) = parse(*kind, body)
                        && capability == CAP_NOTIFICATIONS
                        && let Some(l) = self.layers.get_mut(&id)
                    {
                        l.granted = grant;
                    }
                }
                Event::Sealed { kind, body } if *kind == KIND_NOTIFICATION => {
                    let Ok(Frame::Notification(text)) = parse(*kind, body) else {
                        continue;
                    };
                    if !self.granted(id) {
                        out.push(self.drop_with(Dropped::Denied));
                        continue;
                    }
                    let outcome = self.notify(now_ms, id, Source::Sealed, &text);
                    if let Outcome::Os(note) = &outcome {
                        sink.send(note);
                    }
                    out.push(outcome);
                }
                // A truncated string is a notice for the window, not a notification.
                Event::Sealed { .. } | Event::StringTruncated { .. } => {}
            }
        }
        out
    }
}

/// The text of a notification: the fixed prefix and the pane label, and the
/// program's cleaned text when there is some.
#[must_use]
pub fn compose(label: &str, program_text: Option<&str>) -> String {
    let mut text = format!("Program in {label} says:");
    if let Some(t) = program_text {
        let cleaned = clean(t);
        if !cleaned.is_empty() {
            text.push(' ');
            text.push_str(&cleaned);
        }
    }
    text
}

/// Escape what a notification API may read as markup.
#[must_use]
pub fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn removed(c: char) -> bool {
    matches!(
        get_general_category(c),
        Gc::Control
            | Gc::Format
            | Gc::Surrogate
            | Gc::PrivateUse
            | Gc::Unassigned
            | Gc::LineSeparator
            | Gc::ParagraphSeparator
    ) || matches!(u32::from(c), 0x115f | 0x1160 | 0x3164 | 0xffa0)
}

fn is_mark(c: char) -> bool {
    matches!(
        get_general_category(c),
        Gc::NonspacingMark | Gc::SpacingMark | Gc::EnclosingMark
    )
}

/// The program's text under the rules of the Notifications section: characters
/// of the removed categories and the listed fillers are dropped, at most one
/// combining mark is kept per base character and a mark with no base is
/// dropped, and the result is capped at 200 bytes and 60 terminal cells, ending
/// in a visible marker if it was cut.
#[must_use]
pub fn clean(text: &str) -> String {
    let mut kept = String::new();
    let mut have_base = false;
    let mut marks = 0;
    for c in text.chars() {
        // A code point that draws nothing is shown as a visible escape, so the
        // reader can see it is there; whatever else is removed stays removed.
        if cfg!(feature = "zero-width-text")
            && (lumen_track0::is_default_ignorable(c) || lumen_track0::is_bidi(c))
        {
            kept.push_str(&lumen_track0::escape_invisible(c.encode_utf8(&mut [0; 4])));
            have_base = true;
            marks = 0;
            continue;
        }
        if removed(c) {
            continue;
        }
        if is_mark(c) {
            if !have_base || marks >= 1 {
                continue;
            }
            marks += 1;
        } else {
            have_base = true;
            marks = 0;
        }
        kept.push(c);
    }
    let cells = |s: &str| -> usize { s.chars().map(|c| grapheme_display_width(c, &[])).sum() };
    if kept.len() <= MAX_TEXT_BYTES && cells(&kept) <= MAX_TEXT_CELLS {
        return kept;
    }
    // Cut so that the text and its marker stay inside both caps.
    let room_bytes = MAX_TEXT_BYTES - MARKER.len_utf8();
    let room_cells = MAX_TEXT_CELLS - grapheme_display_width(MARKER, &[]);
    let mut out = String::new();
    let mut used = 0;
    for c in kept.chars() {
        let w = grapheme_display_width(c, &[]);
        if out.len() + c.len_utf8() > room_bytes || used + w > room_cells {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push(MARKER);
    out
}
