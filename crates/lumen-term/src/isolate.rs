//! Grid parser isolation: what a grid block's emulator is never given.
//!
//! The filter sits between the arm layer and the emulator, after the frame
//! detector, so the one pipeline is detector, then this filter, then the VT
//! parser. In grid mode it removes:
//!
//! - every string-type escape other than an allowlisted OSC: DCS, APC, PM and
//!   SOS, in their 7-bit forms (`ESC P`, `ESC _`, `ESC ^`, `ESC X`);
//! - every 8-bit C1 control (0x80 to 0x9F), by byte and by code point, so the
//!   UTF-8 form of U+0080 to U+009F (C2 80 to C2 9F) goes too. A byte in that
//!   range that continues a UTF-8 sequence is text and stays;
//! - invalid UTF-8, so the filter and the emulator agree on what a code point is;
//! - OSC other than the allowlist: title (0, 1, 2) and colours (4, 10 to 19,
//!   104, 110 to 119). OSC 8, 52 and 133 have no protocol effect here.
//!
//! The only string terminator is `ESC \`, and BEL for an allowlisted OSC. The
//! 8-bit ST, CAN and SUB do not end a string. A string is read up to 4 KiB; at
//! the cap the filter discards bytes until `ESC \` and reports one
//! [`Note::StringTruncated`], so neither the rest of the string nor later
//! output gets VT effects from it. There is no second cap that resumes parsing.
//!
//! The filter holds partial state across calls, so a sequence split at any
//! read boundary is treated as if it came whole.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// The most bytes of one string that are read before the rest is discarded.
pub const STRING_CAP: usize = 4096;

/// Something the filter tells the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// A string reached [`STRING_CAP`] and the rest of it is being discarded.
    StringTruncated,
    /// An allowed OSC sequence that is exactly one colour query.
    ColourQuery(crate::reply::ColourQuery),
}

/// What the filter has removed so far, for tests and the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// C1 controls removed, by byte or by code point.
    pub c1: u32,
    /// DCS, APC, PM and SOS strings removed.
    pub strings: u32,
    /// OSC outside the allowlist removed.
    pub osc: u32,
    /// Bytes of invalid UTF-8 removed.
    pub invalid: u32,
    /// Strings that reached the cap.
    pub truncated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Reading an OSC body; `esc` is set after an ESC.
    Osc {
        body: Vec<u8>,
        esc: bool,
    },
    /// Discarding a string until `ESC \`; `read` bytes so far.
    Skip {
        esc: bool,
        read: usize,
    },
}

/// The streaming filter.
#[derive(Debug, Clone)]
pub struct Isolator {
    state: State,
    /// The bytes of the UTF-8 sequence in progress.
    pending: Vec<u8>,
    stats: Stats,
}

impl Default for Isolator {
    fn default() -> Self {
        Self::new()
    }
}

fn allowed_osc(body: &[u8]) -> bool {
    // An ESC or another control inside the body would end the string in a
    // parser that does not follow the one-terminator rule, so such a body is
    // not passed on.
    if body.iter().any(|b| *b < 0x20 || *b == 0x7f) {
        return false;
    }
    let digits: Vec<u8> = body
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    let rest = &body[digits.len()..];
    if !(rest.is_empty() || rest[0] == b';') || digits.is_empty() || digits.len() > 3 {
        return false;
    }
    let number = digits
        .iter()
        .fold(0u32, |n, d| n * 10 + u32::from(d - b'0'));
    matches!(number, 0..=2 | 4 | 10..=19 | 104 | 110..=119)
        && digits.len() == number.to_string().len()
}

fn sequence_length(lead: u8) -> usize {
    match lead {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn second_byte_ok(lead: u8, b: u8) -> bool {
    match lead {
        0xe0 => (0xa0..=0xbf).contains(&b),
        0xed => (0x80..=0x9f).contains(&b),
        0xf0 => (0x90..=0xbf).contains(&b),
        0xf4 => (0x80..=0x8f).contains(&b),
        _ => (0x80..=0xbf).contains(&b),
    }
}

impl Isolator {
    /// A filter in the ground state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            pending: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// What the filter has removed so far.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Filter `bytes`. What may reach the emulator is appended to `out`, and
    /// anything the terminal should show is appended to `notes`.
    pub fn filter(&mut self, bytes: &[u8], out: &mut Vec<u8>, notes: &mut Vec<Note>) {
        for &b in bytes {
            self.step(b, out, notes);
        }
    }

    /// The stream ended: what is held back is dropped, since none of it is a
    /// complete, allowed sequence.
    pub fn finish(&mut self) {
        if !self.pending.is_empty() {
            self.stats.invalid += 1;
            self.pending.clear();
        }
        self.state = State::Ground;
    }

    fn step(&mut self, b: u8, out: &mut Vec<u8>, notes: &mut Vec<Note>) {
        match std::mem::replace(&mut self.state, State::Ground) {
            State::Ground => self.ground(b, out),
            State::Esc => self.after_esc(b, out),
            State::Osc { body, esc } => self.osc(body, esc, b, out, notes),
            State::Skip { esc, read } => self.skip(esc, read, b, notes),
        }
    }

    fn ground(&mut self, b: u8, out: &mut Vec<u8>) {
        if b == ESC {
            self.abandon_utf8();
            self.state = State::Esc;
        } else {
            self.text(b, out);
        }
    }

    fn after_esc(&mut self, b: u8, out: &mut Vec<u8>) {
        match b {
            b'P' | b'_' | b'^' | b'X' => {
                self.stats.strings += 1;
                self.state = State::Skip {
                    esc: false,
                    read: 0,
                };
            }
            b']' => {
                self.state = State::Osc {
                    body: Vec::new(),
                    esc: false,
                };
            }
            ESC => self.state = State::Esc,
            0x80..=0xff => {
                // ESC then a byte that is not ASCII is not an escape sequence.
                // The ESC goes, so that if the byte is dropped (a C1 control or
                // invalid UTF-8) the next byte is not read as following an ESC.
                self.text(b, out);
            }
            _ => {
                out.push(ESC);
                self.ground(b, out);
            }
        }
    }

    fn osc(
        &mut self,
        mut body: Vec<u8>,
        esc: bool,
        b: u8,
        out: &mut Vec<u8>,
        notes: &mut Vec<Note>,
    ) {
        let terminator: Option<&[u8]> = if esc && b == b'\\' {
            Some(&[ESC, b'\\'])
        } else if !esc && b == BEL {
            Some(&[BEL])
        } else {
            None
        };
        if let Some(end) = terminator {
            if allowed_osc(&body) {
                if let Some(query) = crate::reply::parse_query(&body) {
                    notes.push(Note::ColourQuery(query));
                }
                out.extend_from_slice(&[ESC, b']']);
                out.extend_from_slice(&body);
                out.extend_from_slice(end);
            } else {
                self.stats.osc += 1;
            }
            self.abandon_utf8();
            return;
        }
        // An ESC that did not start the terminator is part of the body.
        if esc {
            body.push(ESC);
        }
        if b == ESC {
            self.abandon_utf8();
        } else {
            self.text(b, &mut body);
        }
        if body.len() > STRING_CAP {
            self.stats.truncated += 1;
            notes.push(Note::StringTruncated);
            self.state = State::Skip {
                esc: b == ESC,
                read: body.len(),
            };
        } else {
            self.state = State::Osc {
                body,
                esc: b == ESC,
            };
        }
    }

    fn skip(&mut self, esc: bool, read: usize, b: u8, notes: &mut Vec<Note>) {
        if esc && b == b'\\' {
            return;
        }
        // An ESC counts once it is known not to start the terminator.
        let now = read + usize::from(esc) + usize::from(b != ESC);
        if read <= STRING_CAP && now > STRING_CAP {
            self.stats.truncated += 1;
            notes.push(Note::StringTruncated);
        }
        self.state = State::Skip {
            esc: b == ESC,
            read: now,
        };
    }

    fn abandon_utf8(&mut self) {
        if !self.pending.is_empty() {
            self.stats.invalid += 1;
            self.pending.clear();
        }
    }

    /// One byte of text: pass it on, hold it for UTF-8 validation, or drop it.
    fn text(&mut self, b: u8, out: &mut Vec<u8>) {
        if let Some(&lead) = self.pending.first() {
            let ok = if self.pending.len() == 1 {
                second_byte_ok(lead, b)
            } else {
                (0x80..=0xbf).contains(&b)
            };
            if !ok {
                self.abandon_utf8();
                self.text(b, out);
                return;
            }
            self.pending.push(b);
            if self.pending.len() == sequence_length(lead) {
                let c1 = lead == 0xc2 && b <= 0x9f;
                if c1 {
                    self.stats.c1 += 1;
                } else {
                    out.extend_from_slice(&self.pending);
                }
                self.pending.clear();
            }
            return;
        }
        match b {
            0x00..=0x7f => out.push(b),
            0x80..=0x9f => self.stats.c1 += 1,
            0xa0..=0xc1 | 0xf5..=0xff => self.stats.invalid += 1,
            _ => self.pending.push(b),
        }
    }
}
