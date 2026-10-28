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
    /// A frame header that was refused (over the length limit, or refused by a
    /// [`HeaderCheck`]): the introducer and header bytes, for the caller to
    /// account for. The header failed, so no frame exists. The introducer is
    /// dropped, never sent to the VT parser. The rest of the bytes follow as
    /// [`Event::Output`], and the detector resumes at the byte after the
    /// introducer, so a frame hidden in the rest of the header or the body is
    /// still found. The declared length is never used to skip input.
    Rejected(Vec<u8>),
    /// The stream ended after a complete header but before the whole payload.
    /// The bytes are the payload received so far. A shorter, partial introducer
    /// or header is released as `Output` instead.
    Truncated(Vec<u8>),
}

/// Lets the caller accept or refuse a frame from its first bytes, before the
/// detector commits to the frame's declared length.
///
/// The declared length is unauthenticated until a header has been verified. A
/// detector that trusted it would swallow that many bytes after a forged
/// header, hiding any real frame inside. With a check, a refused candidate is
/// not a frame: the detector reports it as [`Event::Rejected`] and resumes
/// scanning at the byte after the introducer.
pub trait HeaderCheck {
    /// How many leading payload bytes [`accept`](Self::accept) needs for frames
    /// of this type byte. Zero means the frame is accepted without a check.
    fn prefix_len(&self, kind: u8) -> usize;

    /// Decide from the first [`prefix_len`](Self::prefix_len) payload bytes
    /// whether the frame is accepted. `declared` is the declared payload length.
    fn accept(&mut self, kind: u8, declared: usize, prefix: &[u8]) -> bool;
}

/// A check that accepts every frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCheck;

impl HeaderCheck for NoCheck {
    fn prefix_len(&self, _kind: u8) -> usize {
        0
    }

    fn accept(&mut self, _kind: u8, _declared: usize, _prefix: &[u8]) -> bool {
        true
    }
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
        /// The declared payload length.
        declared: usize,
        /// Payload bytes still to be checked before the frame is accepted.
        /// Zero once accepted, or when no check applies.
        need: usize,
    },
}

/// How a pass over some input ended.
enum Pass {
    /// All input was used.
    Done,
    /// A candidate was refused. `rescan` holds the bytes after its introducer,
    /// and the first `consumed` bytes of the input are already inside it.
    Refused { rescan: Vec<u8>, consumed: usize },
}

/// The stateful detector.
#[derive(Debug)]
pub struct Detector {
    state: State,
    /// Candidate bytes, or the payload collected so far.
    buf: Vec<u8>,
    /// The introducer and header of the frame being collected.
    head: Vec<u8>,
    /// Continuation bytes still expected in a UTF-8 sequence in ordinary
    /// output. A raw `0x9F` inside one is text, not an introducer.
    utf8_cont: u8,
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
            head: Vec::new(),
            utf8_cont: 0,
            max_payload,
        }
    }

    /// Feed one read of PTY bytes and append what was found to `out`. Every
    /// frame is accepted as soon as its declared length is within the limit.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) {
        self.feed_checked(data, out, &mut NoCheck);
    }

    /// Like [`feed`](Self::feed), but `check` decides whether each frame is
    /// accepted from its first bytes.
    pub fn feed_checked(&mut self, data: &[u8], out: &mut Vec<Event>, check: &mut dyn HeaderCheck) {
        let mut plain = Vec::new();
        self.consume(data, &mut plain, out, check);
        flush(&mut plain, out);
    }

    /// Run `data` through the state machine. `plain` collects ordinary bytes
    /// that have not been emitted yet, so they stay in order with frames.
    fn consume(
        &mut self,
        data: &[u8],
        plain: &mut Vec<u8>,
        out: &mut Vec<Event>,
        check: &mut dyn HeaderCheck,
    ) {
        let mut rest = data;
        loop {
            match self.run(rest, plain, out, check) {
                Pass::Done => return,
                Pass::Refused { rescan, consumed } => {
                    rest = &rest[consumed..];
                    // The rescan is short and shrinks at every refusal, so
                    // this loop is bounded and needs no recursion.
                    let mut pending = rescan;
                    while !pending.is_empty() {
                        match self.run(&pending, plain, out, check) {
                            Pass::Done => pending.clear(),
                            Pass::Refused { rescan, consumed } => {
                                let mut next = rescan;
                                next.extend_from_slice(&pending[consumed..]);
                                pending = next;
                            }
                        }
                    }
                }
            }
        }
    }

    fn run(
        &mut self,
        data: &[u8],
        plain: &mut Vec<u8>,
        out: &mut Vec<Event>,
        check: &mut dyn HeaderCheck,
    ) -> Pass {
        let mut i = 0;
        while i < data.len() {
            match self.state {
                State::Idle => {
                    let start = i;
                    while i < data.len() {
                        let b = data[i];
                        if self.utf8_cont > 0 && (0x80..=0xbf).contains(&b) {
                            self.utf8_cont -= 1;
                        } else if matches!(b, 0x1b | 0xc2 | 0x9f) {
                            self.utf8_cont = 0;
                            break;
                        } else {
                            self.utf8_cont = match b {
                                0xc3..=0xdf => 1,
                                0xe0..=0xef => 2,
                                0xf0..=0xf4 => 3,
                                _ => 0,
                            };
                        }
                        i += 1;
                    }
                    plain.extend_from_slice(&data[start..i]);
                    if i < data.len() {
                        self.buf.clear();
                        self.buf.push(data[i]);
                        i += 1;
                        self.state = State::Candidate;
                        if let Some(rescan) = self.settle(plain, out, check) {
                            return Pass::Refused {
                                rescan,
                                consumed: i,
                            };
                        }
                    }
                }
                State::Candidate => {
                    self.buf.push(data[i]);
                    i += 1;
                    if let Some(rescan) = self.settle(plain, out, check) {
                        return Pass::Refused {
                            rescan,
                            consumed: i,
                        };
                    }
                }
                State::Payload {
                    kind,
                    remaining,
                    declared,
                    need,
                } => {
                    // While a check is pending, never read past the bytes it
                    // needs, so a refused candidate leaves only a short rescan.
                    let limit = if need > 0 {
                        need - self.buf.len()
                    } else {
                        remaining
                    };
                    let take = remaining.min(limit).min(data.len() - i);
                    self.buf.extend_from_slice(&data[i..i + take]);
                    i += take;
                    let remaining = remaining - take;
                    let mut need = need;
                    if need > 0 && self.buf.len() >= need {
                        if !check.accept(kind, declared, &self.buf[..need]) {
                            let intro = introducer_len(&self.head).unwrap_or(0);
                            flush(plain, out);
                            out.push(Event::Rejected(self.head.clone()));
                            // The introducer is withheld from the VT parser.
                            let mut rescan = self.head.split_off(intro);
                            rescan.append(&mut self.buf);
                            self.head.clear();
                            self.state = State::Idle;
                            return Pass::Refused {
                                rescan,
                                consumed: i,
                            };
                        }
                        need = 0;
                    }
                    if remaining == 0 {
                        flush(plain, out);
                        out.push(Event::Frame {
                            kind,
                            payload: std::mem::take(&mut self.buf),
                        });
                        self.head.clear();
                        self.state = State::Idle;
                    } else {
                        self.state = State::Payload {
                            kind,
                            remaining,
                            declared,
                            need,
                        };
                    }
                }
            }
        }
        Pass::Done
    }

    /// Signal the end of the stream. A frame whose header was read is reported
    /// as truncated. A partial introducer or header is released as ordinary
    /// output.
    pub fn finish(&mut self, out: &mut Vec<Event>) {
        let buf = std::mem::take(&mut self.buf);
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => {}
            State::Candidate => {
                // Withhold a complete introducer; release the rest.
                let intro = introducer_len(&buf).unwrap_or(0);
                if buf.len() > intro {
                    out.push(Event::Output(buf[intro..].to_vec()));
                }
            }
            State::Payload { .. } => out.push(Event::Truncated(buf)),
        }
    }

    /// Resolve the candidate in `buf` after a byte was added. A candidate that
    /// cannot become a frame is released as output, and scanning resumes at the
    /// byte after its first byte, so an introducer inside it is still found.
    /// Returns bytes to rescan when a complete header was refused.
    fn settle(
        &mut self,
        plain: &mut Vec<u8>,
        out: &mut Vec<Event>,
        check: &mut dyn HeaderCheck,
    ) -> Option<Vec<u8>> {
        if is_candidate_prefix(&self.buf) {
            return self.try_header(plain, out, check);
        }
        // A complete introducer is withheld from the VT parser. A prefix that
        // is not an introducer (such as `ESC [`) is ordinary output.
        let intro = introducer_len(&self.buf).unwrap_or(1);
        let rest = self.buf.split_off(intro);
        if introducer_len(&self.buf).is_none() {
            plain.append(&mut self.buf);
        } else {
            self.buf.clear();
        }
        self.state = State::Idle;
        // `rest` is shorter than the candidate was, so this recursion is bounded.
        self.consume(&rest, plain, out, check);
        None
    }

    /// If the candidate holds a full introducer and header, act on the header.
    fn try_header(
        &mut self,
        plain: &mut Vec<u8>,
        out: &mut Vec<Event>,
        check: &mut dyn HeaderCheck,
    ) -> Option<Vec<u8>> {
        if !matches!(self.state, State::Candidate) {
            return None;
        }
        let intro = introducer_len(&self.buf)?;
        if self.buf.len() < intro + HEADER_LEN {
            return None;
        }
        let h = &self.buf[intro + TAG.len()..intro + HEADER_LEN];
        let kind = h[0];
        let declared = u32::from_be_bytes([h[1], h[2], h[3], h[4]]);
        flush(plain, out);
        let declared = usize::try_from(declared).unwrap_or(usize::MAX);
        let need = check.prefix_len(kind);
        // A frame too short to hold the bytes its check needs cannot pass it.
        if declared > self.max_payload || (need > 0 && declared < need) {
            let header = std::mem::take(&mut self.buf);
            out.push(Event::Rejected(header.clone()));
            self.state = State::Idle;
            return Some(header[intro..].to_vec());
        }
        if declared == 0 {
            self.buf.clear();
            out.push(Event::Frame {
                kind,
                payload: Vec::new(),
            });
            self.state = State::Idle;
        } else {
            self.head = std::mem::take(&mut self.buf);
            self.state = State::Payload {
                kind,
                remaining: declared,
                declared,
                need,
            };
        }
        None
    }
}

/// Move pending plain bytes into `out` as one event.
fn flush(plain: &mut Vec<u8>, out: &mut Vec<Event>) {
    if !plain.is_empty() {
        out.push(Event::Output(std::mem::take(plain)));
    }
}
