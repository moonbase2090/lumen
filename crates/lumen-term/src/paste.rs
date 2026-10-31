//! What a paste needs before it reaches a program.
//!
//! [`judge`] sanitises the text first: the bracketed-paste end marker, ESC, C0
//! controls other than tab and newline, DEL and all C1 controls are removed. A
//! paste that contains a frame introducer is rejected and is never parsed as a
//! frame. Pasted command text then has the bidirectional controls and the
//! invisible characters stripped and counted, as copied command text does.
//!
//! If the target grid has not enabled bracketed paste (DECSET 2004), a paste
//! containing a line break or any other control is held for a confirmation that
//! shows the whole text with the controls visible, and the user may paste it as
//! a single line instead. Nothing here draws the confirmation or writes to a
//! program.

use crate::copy::{Kind, strip};
use std::fmt::Write as _;

/// The bracketed-paste end marker.
const END_MARKER: &str = "\x1b[201~";

/// A paste that is safe to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pasted {
    /// The text to send.
    pub text: String,
    /// Control characters removed.
    pub controls_removed: usize,
    /// Bidirectional controls and invisible characters stripped, to be shown.
    pub stripped: usize,
}

/// Why a paste was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// The paste contains a frame introducer. A notice is shown and the text is
    /// never parsed as a frame.
    FrameIntroducer,
}

/// A paste held for a confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    /// The whole text with every control character shown as a visible escape,
    /// line breaks followed by a real line break so the layout stays readable.
    pub preview: String,
    /// The paste as a single line: each line break becomes one space.
    pub single_line: Pasted,
    /// The paste with its line breaks and other controls kept, apart from ESC
    /// and the C1 controls, which are never kept.
    pub as_written: Pasted,
}

/// What to do with a paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Send this.
    Send(Pasted),
    /// Hold for a confirmation.
    Hold(Hold),
    /// Do not send, and say why.
    Reject(Rejection),
}

fn is_c0_other(c: char) -> bool {
    let n = u32::from(c);
    (n < 0x20 && c != '\t' && c != '\n') || n == 0x7f
}

fn is_c1(c: char) -> bool {
    (0x80..=0x9f).contains(&u32::from(c))
}

/// Remove ESC, the C0 controls other than tab and newline, DEL and the C1
/// controls (after the end marker). `keep` also keeps the C0 controls other than
/// ESC, for the "as written" choice. Returns the text and the number removed.
fn sanitise(text: &str, keep: bool) -> (String, usize) {
    let without_marker = text.replace(END_MARKER, "");
    let mut removed = (text.len() - without_marker.len()) / END_MARKER.len();
    let mut out = String::with_capacity(without_marker.len());
    for c in without_marker.chars() {
        let drop = c == '\x1b' || is_c1(c) || (!keep && is_c0_other(c));
        if drop {
            removed += 1;
        } else {
            out.push(c);
        }
    }
    (out, removed)
}

fn finish(text: &str, keep: bool) -> Pasted {
    let (clean, controls_removed) = sanitise(text, keep);
    let (text, stripped) = strip(&clean, Kind::Command);
    Pasted {
        text,
        controls_removed,
        stripped,
    }
}

/// Every control character as `⟨U+XXXX⟩`, except tab; a line feed is followed by
/// a real line break so the lines stay readable.
fn preview(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c == '\t' {
            out.push(c);
        } else if c.is_control() {
            let _ = write!(out, "\u{27e8}U+{:04X}\u{27e9}", u32::from(c));
            if c == '\n' {
                out.push('\n');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Decide what to do with `text`, pasted into a grid whose bracketed-paste mode
/// is `bracketed`.
#[must_use]
pub fn judge(text: &str, bracketed: bool) -> Decision {
    // Any APC introducer is a frame introducer here: a paste has no reason to
    // carry one, and rejecting it whole is the safe side.
    if text.contains("\x1b_") {
        return Decision::Reject(Rejection::FrameIntroducer);
    }
    if bracketed {
        return Decision::Send(finish(text, false));
    }
    let has_line_break = text.contains(['\n', '\r']);
    let has_control = text
        .chars()
        .any(|c| is_c0_other(c) || is_c1(c) || c == '\x1b');
    if !has_line_break && !has_control {
        return Decision::Send(finish(text, false));
    }
    let single = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    Decision::Hold(Hold {
        preview: preview(text),
        single_line: finish(&single, false),
        as_written: finish(text, true),
    })
}
