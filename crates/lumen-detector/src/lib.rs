//! Frame detector.
//!
//! The detector sees every PTY byte before the VT parser. It removes frame
//! bytes from the stream and passes everything else on unchanged. Its state
//! persists across reads, so an introducer split over two reads is reassembled.
//!
//! The wire layout here is the phase 0.5 experiment profile, not a normative
//! encoding. A frame is an introducer, an 8-byte tag, a one-byte type, a
//! four-byte big-endian payload length and the payload. The introducer is
//! `ESC _`, the C1 byte `0x9F`, or the UTF-8 form of that C1 byte (`0xC2 0x9F`).
//!
//! This crate only finds frames. It does not authenticate or interpret them.

#![forbid(unsafe_code)]

/// Fixed tag that follows an introducer (experiment profile value).
pub const TAG: [u8; 8] = *b"\x8aLUMEN\x01\xf3";

/// Largest declared payload accepted by default: the 1 MiB frame size limit.
pub const DEFAULT_MAX_PAYLOAD: usize = 1 << 20;

/// Length of the header after the introducer: tag, type byte, length field.
const HEADER_LEN: usize = TAG.len() + 1 + 4;

/// Something the detector found in the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Ordinary bytes, in order, for the VT parser.
    Output(Vec<u8>),
    /// A complete frame.
    Frame {
        /// The frame type byte.
        kind: u8,
        /// The payload, exactly as long as the declared length.
        payload: Vec<u8>,
    },
    /// A frame header whose declared length is over the limit: the introducer
    /// and header bytes. The header failed, so no frame exists. The detector
    /// resumes at the byte after the introducer, so a frame hidden in the rest
    /// of the header or the body is still found, and the other bytes arrive as
    /// ordinary output. The declared length is never used to skip input.
    Rejected(Vec<u8>),
    /// The stream ended after a complete header but before the whole payload.
    /// The bytes are the payload received so far. A shorter, partial introducer
    /// or header is released as `Output` instead.
    Truncated(Vec<u8>),
}

#[derive(Debug)]
enum State {
    Idle,
    /// Collecting a candidate: introducer, tag and header.
    Candidate,
    /// Collecting `remaining` more payload bytes.
    Payload {
        kind: u8,
        remaining: usize,
    },
}

/// The stateful detector.
#[derive(Debug)]
pub struct Detector {
    state: State,
    /// Candidate bytes, or the payload collected so far.
    buf: Vec<u8>,
    max_payload: usize,
}

impl Default for Detector {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PAYLOAD)
    }
}

/// Length of the introducer that `bytes` starts with, if it is complete.
/// `None` means `bytes` is not (yet) a complete introducer.
fn introducer_len(bytes: &[u8]) -> Option<usize> {
    match bytes {
        [0x1b, b'_', ..] | [0xc2, 0x9f, ..] => Some(2),
        [0x9f, ..] => Some(1),
        _ => None,
    }
}

/// Whether `bytes` could still become an introducer plus a matching header.
fn is_candidate_prefix(bytes: &[u8]) -> bool {
    let Some(first) = bytes.first() else {
        return true;
    };
    let intro = match first {
        0x1b => {
            if bytes.len() == 1 {
                return true;
            }
            if bytes[1] != b'_' {
                return false;
            }
            2
        }
        0xc2 => {
            if bytes.len() == 1 {
                return true;
            }
            if bytes[1] != 0x9f {
                return false;
            }
            2
        }
        0x9f => 1,
        _ => return false,
    };
    let after = &bytes[intro..];
    let n = after.len().min(TAG.len());
    after[..n] == TAG[..n]
}

impl Detector {
    /// A detector that rejects declared payloads longer than `max_payload`.
    #[must_use]
    pub fn new(max_payload: usize) -> Self {
        Self {
            state: State::Idle,
            buf: Vec::new(),
            max_payload,
        }
    }

    /// Feed one read of PTY bytes and append what was found to `out`.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) {
        let mut plain = Vec::new();
        self.consume(data, &mut plain, out);
        flush(&mut plain, out);
    }

    /// Run `data` through the state machine. `plain` collects ordinary bytes
    /// that have not been emitted yet, so they stay in order with frames.
    fn consume(&mut self, data: &[u8], plain: &mut Vec<u8>, out: &mut Vec<Event>) {
        let mut i = 0;
        while i < data.len() {
            match self.state {
                State::Idle => {
                    let start = i;
                    while i < data.len() && !matches!(data[i], 0x1b | 0xc2 | 0x9f) {
                        i += 1;
                    }
                    plain.extend_from_slice(&data[start..i]);
                    if i < data.len() {
                        self.buf.clear();
                        self.buf.push(data[i]);
                        i += 1;
                        self.state = State::Candidate;
                        self.settle(plain, out);
                    }
                }
                State::Candidate => {
                    self.buf.push(data[i]);
                    i += 1;
                    self.settle(plain, out);
                }
                State::Payload { kind, remaining } => {
                    let take = remaining.min(data.len() - i);
                    self.buf.extend_from_slice(&data[i..i + take]);
                    i += take;
                    if take == remaining {
                        flush(plain, out);
                        out.push(Event::Frame {
                            kind,
                            payload: std::mem::take(&mut self.buf),
                        });
                        self.state = State::Idle;
                    } else {
                        self.state = State::Payload {
                            kind,
                            remaining: remaining - take,
                        };
                    }
                }
            }
        }
    }

    /// Signal the end of the stream. A frame whose header was read is reported
    /// as truncated. A partial introducer or header is released as ordinary
    /// output.
    pub fn finish(&mut self, out: &mut Vec<Event>) {
        let buf = std::mem::take(&mut self.buf);
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => {}
            State::Candidate => out.push(Event::Output(buf)),
            State::Payload { .. } => out.push(Event::Truncated(buf)),
        }
    }

    /// Resolve the candidate in `buf` after a byte was added. A candidate that
    /// cannot become a frame is released as output, and scanning resumes at the
    /// byte after its first byte, so an introducer inside it is still found.
    fn settle(&mut self, plain: &mut Vec<u8>, out: &mut Vec<Event>) {
        if is_candidate_prefix(&self.buf) {
            self.try_header(plain, out);
            return;
        }
        let rest = self.buf.split_off(1);
        plain.append(&mut self.buf);
        self.state = State::Idle;
        // `rest` is shorter than the candidate was, so this recursion is bounded.
        self.consume(&rest, plain, out);
    }

    /// If the candidate holds a full introducer and header, act on the header.
    fn try_header(&mut self, plain: &mut Vec<u8>, out: &mut Vec<Event>) {
        if !matches!(self.state, State::Candidate) {
            return;
        }
        let Some(intro) = introducer_len(&self.buf) else {
            return;
        };
        if self.buf.len() < intro + HEADER_LEN {
            return;
        }
        let h = &self.buf[intro + TAG.len()..intro + HEADER_LEN];
        let kind = h[0];
        let declared = u32::from_be_bytes([h[1], h[2], h[3], h[4]]);
        flush(plain, out);
        let too_big = usize::try_from(declared).map_or(true, |n| n > self.max_payload);
        if too_big {
            let header = std::mem::take(&mut self.buf);
            out.push(Event::Rejected(header.clone()));
            self.state = State::Idle;
            // Resume after the introducer. `header` is shorter than the
            // candidate was, so this recursion is bounded.
            self.consume(&header[intro..], plain, out);
        } else if declared == 0 {
            self.buf.clear();
            out.push(Event::Frame {
                kind,
                payload: Vec::new(),
            });
            self.state = State::Idle;
        } else {
            self.buf.clear();
            self.state = State::Payload {
                kind,
                remaining: declared as usize,
            };
        }
    }
}

/// Move pending plain bytes into `out` as one event.
fn flush(plain: &mut Vec<u8>, out: &mut Vec<Event>) {
    if !plain.is_empty() {
        out.push(Event::Output(std::mem::take(plain)));
    }
}
