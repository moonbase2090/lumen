//! The notification guard: removes OSC 9, OSC 777 and OSC 99 from the bytes a
//! layer passes on, before any emulator sees them.
//!
//! The guard is a streaming filter. It holds a partial introducer or number
//! across calls, so a sequence split at any read boundary is still recognised.
//! A sequence it does not intercept is passed on byte for byte. Only the
//! `ESC ]` introducer is handled: the pinned emulator does not treat the 8-bit
//! C1 form (0x9D, or its UTF-8 encoding) as an OSC introducer, so there is no
//! sequence to intercept there, and passing those bytes on keeps legitimate
//! UTF-8 text intact.
//!
//! The number is read as digits, so `009` is OSC 9. A sequence is intercepted
//! once its number is known to be 9, 99 or 777; its body is dropped up to its
//! terminator (BEL, `ESC \` or the 8-bit ST), and the start of another escape
//! sequence, CAN or SUB ends it too.

/// Digits kept while reading the number; a longer number is not a notification.
const MAX_DIGITS: usize = 8;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const ST_C1: u8 = 0x9c;

/// A notification the guard removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intercepted {
    /// The OSC number: 9, 99 or 777.
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Saw `ESC ]` and these digits.
    Number(Vec<u8>),
    /// Inside an intercepted body; `esc` is set after an ESC.
    Swallow {
        esc: bool,
    },
}

/// The streaming guard.
#[derive(Debug, Clone)]
pub struct Guard {
    state: State,
}

impl Default for Guard {
    fn default() -> Self {
        Self::new()
    }
}

fn is_notification(number: u32) -> bool {
    matches!(number, 9 | 99 | 777)
}

fn number_of(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0u32, |n, d| {
        n.checked_mul(10)?.checked_add(u32::from(d - b'0'))
    })
}

impl Guard {
    /// A guard in the ground state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Ground,
        }
    }

    /// Filter `bytes`. Bytes to pass on are appended to `out`, and each
    /// intercepted notification is appended to `found` as it is recognised.
    pub fn filter(&mut self, bytes: &[u8], out: &mut Vec<u8>, found: &mut Vec<Intercepted>) {
        for &b in bytes {
            self.step(b, out, found);
        }
    }

    /// End of stream: pass on whatever was held back.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        match std::mem::replace(&mut self.state, State::Ground) {
            State::Esc => out.push(ESC),
            State::Number(digits) => {
                out.extend_from_slice(&[ESC, b']']);
                out.extend_from_slice(&digits);
            }
            State::Ground | State::Swallow { .. } => {}
        }
    }

    fn step(&mut self, b: u8, out: &mut Vec<u8>, found: &mut Vec<Intercepted>) {
        match std::mem::replace(&mut self.state, State::Ground) {
            State::Ground => {
                if b == ESC {
                    self.state = State::Esc;
                } else {
                    out.push(b);
                }
            }
            State::Esc => match b {
                b']' => self.state = State::Number(Vec::new()),
                ESC => {
                    out.push(ESC);
                    self.state = State::Esc;
                }
                _ => {
                    out.push(ESC);
                    self.step(b, out, found);
                }
            },
            State::Number(mut digits) => {
                if b.is_ascii_digit() && digits.len() < MAX_DIGITS {
                    digits.push(b);
                    self.state = State::Number(digits);
                } else if let Some(n) = number_of(&digits).filter(|n| is_notification(*n)) {
                    found.push(Intercepted { number: n });
                    self.swallow(b);
                } else {
                    out.extend_from_slice(&[ESC, b']']);
                    out.extend_from_slice(&digits);
                    self.step(b, out, found);
                }
            }
            State::Swallow { esc } => self.swallow_next(esc, b, out, found),
        }
    }

    /// The byte that ended the number of an intercepted sequence.
    fn swallow(&mut self, b: u8) {
        self.state = match b {
            b';' => State::Swallow { esc: false },
            BEL | ST_C1 | CAN | SUB => State::Ground,
            ESC => State::Swallow { esc: true },
            _ => State::Swallow { esc: false },
        };
    }

    fn swallow_next(&mut self, esc: bool, b: u8, out: &mut Vec<u8>, found: &mut Vec<Intercepted>) {
        if esc {
            // ESC \ ends the sequence; any other byte after ESC starts a new
            // escape sequence, which is read afresh.
            if b == b'\\' {
                self.state = State::Ground;
            } else {
                self.state = State::Esc;
                self.step(b, out, found);
            }
            return;
        }
        match b {
            BEL | ST_C1 | CAN | SUB => self.state = State::Ground,
            ESC => self.state = State::Swallow { esc: true },
            _ => self.state = State::Swallow { esc: false },
        }
    }
}
