//! The notification guard: removes OSC 9, OSC 777 and OSC 99 from the bytes a
//! layer passes on, before any emulator sees them.
//!
//! The guard is a streaming filter. It holds a partial introducer or number
//! across calls, so a sequence split at any read boundary is still recognised.
//! A sequence it does not intercept is passed on byte for byte. Three
//! introducers are recognised: `ESC ]`, the raw 8-bit C1 byte 0x9D, and its
//! UTF-8 form C2 9D. The pinned emulator acts on none of the last two, but a
//! terminal that did would, so they are intercepted too. A raw 0x9D that
//! continues an incomplete UTF-8 sequence is text, not an introducer.
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
const C1_OSC: u8 = 0x9d;

/// A notification the guard removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intercepted {
    /// The OSC number: 9, 99 or 777.
    pub number: u32,
}

/// How an OSC introducer was written, so a sequence that is passed on is
/// passed on exactly as it came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intro {
    /// `ESC ]`.
    Esc,
    /// The raw byte 0x9D.
    C1,
    /// The UTF-8 form C2 9D.
    C1Utf8,
}

impl Intro {
    fn bytes(self) -> &'static [u8] {
        match self {
            Intro::Esc => &[ESC, b']'],
            Intro::C1 => &[C1_OSC],
            Intro::C1Utf8 => &[0xc2, C1_OSC],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Saw the byte C2, which may start the UTF-8 form of the C1 introducer.
    C2,
    /// Saw an introducer and these digits.
    Number(Intro, Vec<u8>),
    /// Inside an intercepted body; `esc` is set after an ESC.
    Swallow {
        esc: bool,
    },
}

/// The streaming guard.
#[derive(Debug, Clone)]
pub struct Guard {
    state: State,
    /// Continuation bytes still owed by the UTF-8 sequence in progress.
    utf8_need: u8,
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
            utf8_need: 0,
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
            State::C2 => out.push(0xc2),
            State::Number(intro, digits) => {
                out.extend_from_slice(intro.bytes());
                out.extend_from_slice(&digits);
            }
            State::Ground | State::Swallow { .. } => {}
        }
    }

    fn step(&mut self, b: u8, out: &mut Vec<u8>, found: &mut Vec<Intercepted>) {
        match std::mem::replace(&mut self.state, State::Ground) {
            State::Ground => self.ground(b, out),
            State::C2 => {
                if b == C1_OSC {
                    self.state = State::Number(Intro::C1Utf8, Vec::new());
                } else {
                    // C2 is the lead byte of a two-byte character.
                    out.push(0xc2);
                    self.utf8_need = 1;
                    self.step(b, out, found);
                }
            }
            State::Esc => match b {
                b']' => self.state = State::Number(Intro::Esc, Vec::new()),
                ESC => {
                    out.push(ESC);
                    self.state = State::Esc;
                }
                _ => {
                    out.push(ESC);
                    self.step(b, out, found);
                }
            },
            State::Number(intro, mut digits) => {
                if b.is_ascii_digit() && digits.len() < MAX_DIGITS {
                    digits.push(b);
                    self.state = State::Number(intro, digits);
                } else if let Some(n) = number_of(&digits).filter(|n| is_notification(*n)) {
                    found.push(Intercepted { number: n });
                    self.swallow(b);
                } else {
                    out.extend_from_slice(intro.bytes());
                    out.extend_from_slice(&digits);
                    self.step(b, out, found);
                }
            }
            State::Swallow { esc } => self.swallow_next(esc, b, out, found),
        }
    }

    /// One byte in the ground state.
    fn ground(&mut self, b: u8, out: &mut Vec<u8>) {
        match b {
            ESC => {
                self.utf8_need = 0;
                self.state = State::Esc;
            }
            C1_OSC if self.utf8_need == 0 => {
                self.state = State::Number(Intro::C1, Vec::new());
            }
            0xc2 if self.utf8_need == 0 => self.state = State::C2,
            _ => {
                self.utf8_need = match b {
                    0xf0..=0xff => 3,
                    0xe0..=0xef => 2,
                    0xc0..=0xdf => 1,
                    0x80..=0xbf => self.utf8_need.saturating_sub(1),
                    _ => 0,
                };
                out.push(b);
            }
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
