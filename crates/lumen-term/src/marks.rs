//! The prompt marks a shell writes: OSC 133 `A`, `B`, `C` and `D` and OSC 7.
//!
//! The terminal reads the program's output for them as it arrives and keeps the
//! newest few, for what reads them next and for tests. Noting a mark changes
//! nothing else: the sequences are removed before the emulator as any other
//! OSC is (see [`isolate`](crate::isolate)), and this scanner never hands bytes
//! on.

/// The most marks kept: the newest are, and the oldest go first.
pub const MARKS_KEPT: usize = 256;

/// The longest OSC body looked at, in bytes. A longer string is not a mark.
const BODY_CAP: usize = 4096;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// A prompt mark the shell wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// OSC 133 `A`: the prompt starts.
    PromptStart,
    /// OSC 133 `B`: the prompt ends and the command is typed.
    PromptEnd,
    /// OSC 133 `C`: the command starts running.
    CommandStart,
    /// OSC 133 `D`: the command ended, with its exit status when it gave one.
    CommandEnd(Option<i32>),
    /// OSC 7: the working directory, as the URL the shell wrote.
    Directory(String),
}

fn mark_of(body: &[u8]) -> Option<Mark> {
    if let Some(rest) = body.strip_prefix(b"133;") {
        return match rest {
            [b'A', ..] => Some(Mark::PromptStart),
            [b'B', ..] => Some(Mark::PromptEnd),
            [b'C', ..] => Some(Mark::CommandStart),
            [b'D'] => Some(Mark::CommandEnd(None)),
            [b'D', b';', status @ ..] => {
                let digits = status.split(|b| *b == b';').next().unwrap_or_default();
                Some(Mark::CommandEnd(
                    std::str::from_utf8(digits)
                        .ok()
                        .and_then(|s| s.parse().ok()),
                ))
            }
            _ => None,
        };
    }
    body.strip_prefix(b"7;")
        .map(|url| Mark::Directory(String::from_utf8_lossy(url).into_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Reading an OSC body; `esc` is set after an ESC inside it.
    Osc {
        body: Vec<u8>,
        esc: bool,
    },
}

/// Reads output for prompt marks across read boundaries.
#[derive(Debug, Clone)]
pub struct MarkScanner {
    state: State,
    marks: std::collections::VecDeque<Mark>,
}

impl Default for MarkScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkScanner {
    /// A scanner that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            marks: std::collections::VecDeque::new(),
        }
    }

    /// The marks seen, oldest first, up to [`MARKS_KEPT`].
    #[must_use]
    pub fn marks(&self) -> Vec<Mark> {
        self.marks.iter().cloned().collect()
    }

    /// Read `bytes`, the next of the program's output.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let state = std::mem::replace(&mut self.state, State::Ground);
            self.state = match state {
                State::Ground | State::Esc if b == ESC => State::Esc,
                State::Esc if b == b']' => State::Osc {
                    body: Vec::new(),
                    esc: false,
                },
                State::Ground | State::Esc => State::Ground,
                State::Osc { body, esc } => self.in_osc(body, esc, b),
            };
        }
    }

    fn in_osc(&mut self, mut body: Vec<u8>, esc: bool, b: u8) -> State {
        // BEL ends the string, as does ESC followed by a backslash.
        if (esc && b == b'\\') || (!esc && b == BEL) {
            if let Some(mark) = mark_of(&body) {
                if self.marks.len() == MARKS_KEPT {
                    self.marks.pop_front();
                }
                self.marks.push_back(mark);
            }
            return State::Ground;
        }
        if esc {
            body.push(ESC);
        }
        if b == ESC {
            // Wait for what follows it.
            return State::Osc { body, esc: true };
        }
        body.push(b);
        if body.len() > BODY_CAP {
            return State::Ground;
        }
        State::Osc { body, esc: false }
    }
}
