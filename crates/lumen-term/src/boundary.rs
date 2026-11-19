//! Where a sealed shell is between commands, from its sealed boundary frames.
//!
//! On a sealed layer the shell's hooks send a sealed command start (`0x10`)
//! when a command begins, and a command end (`0x12`) and a prompt (`0x11`) when
//! its line editor is about to read again. Only frames that verified and opened
//! reach [`Boundaries::see`]: an escaped frame, or one with the seal tag that
//! did not open, is text and changes nothing here.
//!
//! The input box takes keys on a sealed layer only while the last boundary is a
//! prompt with no command start after it, and no Enter has been forwarded since
//! it. Between an Enter and the shell's next frame the line is the shell's: a
//! frame not yet seen means the box does not take keys. A frame out of order
//! is refused the way the sealed sink refuses it, and the tracker then knows
//! nothing: the box goes back to the rules of a layer without sealing.

use lumen_sealed::types::{Frame, KIND_COMMAND_END, KIND_COMMAND_START, KIND_PROMPT, parse};

/// What [`Boundaries::see`] did with a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// A boundary frame in order.
    Applied,
    /// A boundary frame that did not parse or was out of sequence.
    Refused,
    /// Not a boundary frame.
    Other,
}

/// The boundary frames a sealed layer has sent so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Boundaries {
    /// Command ids handed out so far.
    last_id: u32,
    /// The command that is open, if any.
    open: Option<u32>,
    /// The type of the last boundary frame applied.
    last: Option<u8>,
    /// A boundary frame was refused.
    refused: bool,
    /// An Enter was forwarded to the shell and no boundary frame has come
    /// since.
    line_sent: bool,
}

impl Boundaries {
    /// No boundary seen yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take one sealed frame that opened, of type `kind` with payload `body`.
    pub fn see(&mut self, kind: u8, body: &[u8]) -> Seen {
        if !matches!(kind, KIND_COMMAND_START | KIND_PROMPT | KIND_COMMAND_END) {
            return Seen::Other;
        }
        if self.refused {
            return Seen::Refused;
        }
        let applied = match parse(kind, body) {
            Ok(Frame::CommandStart { id }) if id == self.last_id.wrapping_add(1) => {
                // A start while a command is open ends that one as unknown.
                self.last_id = id;
                self.open = Some(id);
                true
            }
            Ok(Frame::CommandEnd { id, .. }) if self.open == Some(id) => {
                self.open = None;
                true
            }
            Ok(Frame::Prompt) => true,
            _ => false,
        };
        if applied {
            self.last = Some(kind);
            self.line_sent = false;
            Seen::Applied
        } else {
            self.refused = true;
            Seen::Refused
        }
    }

    /// The window forwarded an Enter to the shell while it was in front. Until
    /// the shell's next boundary frame, it has not said where it is.
    pub fn line_sent(&mut self) {
        self.line_sent = true;
    }

    /// No command is running by the frames: an Enter was forwarded and no
    /// frame has come since, or the last frame is not a command start. What is
    /// typed now is for the shell's next line. False once a frame was refused.
    #[must_use]
    pub fn between_commands(&self) -> bool {
        !self.refused && (self.line_sent || self.last != Some(KIND_COMMAND_START))
    }

    /// Whether the shell is at a prompt by its sealed frames: the last boundary
    /// is a prompt and no Enter has been forwarded since. `None` once a
    /// boundary frame was refused, because then the frames say nothing.
    #[must_use]
    pub fn at_prompt(&self) -> Option<bool> {
        (!self.refused).then_some(self.last == Some(KIND_PROMPT) && !self.line_sent)
    }
}
