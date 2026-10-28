//! Arm B: Track 0 rich output on every layer.
//!
//! [`Layer`] is the per-layer pipeline for unsealed (Track 0) output. PTY bytes
//! go through the frame detector first. Ordinary bytes are returned for the VT
//! parser. Track 0 frames become [`Item`]s according to the Track 0 rules:
//!
//! - On the alternate screen, or under tmux or screen, a frame is escaped text.
//! - A block is drawn in the unverified-output container only while the
//!   terminal has opened one. Otherwise it goes to the background strip.
//! - Each producer has a small budget of frames and frame bytes. Past it, the
//!   rest of the command is plain text with one notice, whatever spent it. This
//!   covers only part of MUST 4: block counts, depth, table cells, shaping
//!   cache, patch rate and decoded image bytes are not counted here.
//! - A frame that fails to parse, an unknown or sealed-range type, a rejected
//!   header and a truncated frame are all escaped text and nothing more.
//!
//! Nothing here can open or close the container, draw in the trust margin or
//! cause an effect outside the layer. The caller owns those. This crate does
//! not handle sealed frames; they are the sealed layer's job.

#![forbid(unsafe_code)]

use lumen_detector::{Detector, Event, HeaderCheck};
use lumen_sealed::{HEADER_LEN, Header, HeaderError, Receiver};
use lumen_track0::{Block, escaped_text, parse};
use std::collections::VecDeque;

/// Per-read facts about the layer that change how Track 0 is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The alternate screen is active on this layer.
    pub alt_screen: bool,
    /// `TMUX` or `STY` is present in the layer's environment.
    pub tmux_or_screen: bool,
    /// Monotonic time of this read, in milliseconds. Used for the per-second
    /// verification cap and the pin window.
    pub now_ms: u64,
}

/// Header checks allowed per layer per second.
pub const HEADER_CHECKS_PER_SEC: u32 = 4096;
/// Body AEAD checks allowed per layer per second.
pub const BODY_CHECKS_PER_SEC: u32 = 256;
/// Sealed aborts allowed in [`PIN_WINDOW_MS`] before the layer is pinned.
pub const PIN_AFTER_ABORTS: usize = 3;
/// The pin window, in milliseconds.
pub const PIN_WINDOW_MS: u64 = 60_000;

/// The unauthenticated producer's budget for one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Most frames (of any outcome) before the command degrades to text.
    pub max_frames: usize,
    /// Most frame bytes before the command degrades to text.
    pub max_bytes: usize,
}

impl Budget {
    /// 25% of a full budget, the share of an unauthenticated producer.
    #[must_use]
    pub const fn quarter_of(full_frames: usize, full_bytes: usize) -> Self {
        Self {
            max_frames: full_frames / 4,
            max_bytes: full_bytes / 4,
        }
    }
}

/// How the layer is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// The unauthenticated producer's budget.
    pub budget: Budget,
    /// Show background Track 0 as escaped text instead of in the strip.
    pub background_as_text: bool,
}

/// What the caller should do with a piece of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Ordinary bytes for the VT parser.
    Output(Vec<u8>),
    /// A block for the unverified-output container.
    Block(Block),
    /// A block for the collapsed background strip.
    Background(Block),
    /// Visible escaped text, shown in place. It holds no control character.
    Text(String),
    /// The budget is spent. Shown once per command as a chrome notice.
    Degraded,
    /// A sealed frame whose header verified and whose payload opened.
    Sealed {
        /// The verified header.
        header: Header,
        /// The decrypted payload.
        body: Vec<u8>,
    },
    /// The layer is pinned to grid mode: too many sealed aborts in the window.
    /// Shown once, as a chrome notice. The caller closes open blocks as
    /// ended-unknown and offers the out-of-band restore action.
    Pinned,
}

/// What a layer has seen, for the comparison harness. It only observes: no
/// decision reads it. All counts are exact and start at zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Frame candidates with a complete header: accepted plus refused.
    pub frames_seen: u64,
    /// Frames the detector yielded.
    pub frames_accepted: u64,
    /// Candidates refused; the `refused_*` causes below add up to this.
    pub frames_refused: u64,
    /// Refused: the sealed header did not verify (tag, padding, layer, origin).
    pub refused_header_invalid: u64,
    /// Refused: a replayed or stale sequence number.
    pub refused_replay: u64,
    /// Refused: past the header-check cap for the second.
    pub refused_over_header_cap: u64,
    /// Refused: past the body-check cap for the second.
    pub refused_over_body_cap: u64,
    /// Refused: an authentic header that disagrees with the detector's framing.
    pub refused_length_mismatch: u64,
    /// Refused by the detector itself: over the length limit, or too short to
    /// hold a sealed header.
    pub refused_framing: u64,
    /// Authenticated frames whose body failed the AEAD check.
    pub body_aead_failures: u64,
    /// Streams that ended inside a frame.
    pub truncated: u64,
    /// `Item::Degraded` notices given.
    pub degraded_notices: u64,
    /// `Item::Pinned` notices given.
    pub pinned_notices: u64,
    /// Sealed frames opened.
    pub sealed_opened: u64,
    /// Blocks drawn in the container.
    pub track0_blocks: u64,
    /// Blocks sent to the background strip.
    pub background_blocks: u64,
    /// Items shown as escaped text.
    pub text_items: u64,
}

/// Sealed-frame state that the detector consults before it commits to a
/// frame: the header receiver, the per-second cap and the pin window.
#[derive(Debug, Default)]
struct Gate {
    receiver: Option<Receiver>,
    now_ms: u64,
    window_start_ms: u64,
    header_checks: u32,
    body_checks: u32,
    /// The capability a frame of this type byte is sealed for.
    cap_of: Option<fn(u8) -> u8>,
    aborts: Vec<u64>,
    pinned: bool,
    /// An authenticated abort just pinned the layer; the layer announces it
    /// when it handles the refusal that caused it.
    pin_pending: bool,
    /// Headers that verified, in order, for the frames the detector then yields.
    verified: VecDeque<Header>,
    stats: Counters,
    /// The last refusal came from the gate, which has already counted its cause.
    refusal_noted: bool,
}

impl Gate {
    /// Count the cause of a refusal the gate decided.
    fn refuse(&mut self, cause: fn(&mut Counters) -> &mut u64) {
        *cause(&mut self.stats) += 1;
        self.refusal_noted = true;
    }

    fn count_abort(&mut self, now_ms: u64) {
        self.aborts
            .retain(|&t| now_ms.saturating_sub(t) < PIN_WINDOW_MS);
        self.aborts.push(now_ms);
        if self.aborts.len() > PIN_AFTER_ABORTS && !self.pinned {
            self.pinned = true;
            self.pin_pending = true;
        }
    }
}

impl HeaderCheck for Gate {
    fn prefix_len(&self, kind: u8) -> usize {
        if self.receiver.is_some() && kind < lumen_track0::UNSEALED_MIN {
            HEADER_LEN
        } else {
            0
        }
    }

    fn accept(&mut self, kind: u8, declared: usize, prefix: &[u8]) -> bool {
        if self.pinned {
            // Grid mode: no keys are used. The layer shows the frame as text.
            return true;
        }
        if self.now_ms.saturating_sub(self.window_start_ms) >= 1000 {
            self.window_start_ms = self.now_ms;
            self.header_checks = 0;
            self.body_checks = 0;
        }
        self.header_checks = self.header_checks.saturating_add(1);
        if self.header_checks > HEADER_CHECKS_PER_SEC {
            // Over the cap: not verified, so unauthenticated input. It follows
            // the failed-header rule and never counts toward the pin.
            self.refuse(|s| &mut s.refused_over_header_cap);
            return false;
        }
        if self.body_checks >= BODY_CHECKS_PER_SEC {
            // The body budget for this second is spent. Refusing before the
            // header is verified costs nothing and keeps the sequence number.
            self.refuse(|s| &mut s.refused_over_body_cap);
            return false;
        }
        let Some(receiver) = self.receiver.as_mut() else {
            self.refuse(|s| &mut s.refused_header_invalid);
            return false;
        };
        let header = match receiver.verify(prefix) {
            Ok(header) => header,
            Err(HeaderError::Replay) => {
                self.refuse(|s| &mut s.refused_replay);
                return false;
            }
            Err(_) => {
                self.refuse(|s| &mut s.refused_header_invalid);
                return false;
            }
        };
        // The header is authentic. If the detector's framing disagrees with it,
        // that is an abort of an authenticated frame.
        if header.kind != kind || declared != HEADER_LEN.saturating_add(header.len as usize) {
            self.count_abort(self.now_ms);
            self.refuse(|s| &mut s.refused_length_mismatch);
            return false;
        }
        self.body_checks += 1;
        self.verified.push_back(header);
        true
    }
}

/// The Track 0 pipeline for one layer.
#[derive(Debug)]
pub struct Layer {
    detector: Detector,
    gate: Gate,
    config: Config,
    container_open: bool,
    frames: usize,
    bytes: usize,
    degraded: bool,
    /// The pin notice has been given; frames from here on are text.
    pinned_shown: bool,
}

impl Layer {
    /// A layer with no container open.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            detector: Detector::default(),
            gate: Gate::default(),
            config,
            container_open: false,
            frames: 0,
            bytes: 0,
            degraded: false,
            pinned_shown: false,
        }
    }

    /// Give the layer a header receiver. Only a layer that has sealing keys
    /// has one. Without it, every sealed-range frame is escaped text.
    /// `cap_of` says which capability a frame of a given type byte is sealed
    /// for; the capability is bound into the payload's associated data.
    pub fn attach_sealed(&mut self, receiver: Receiver, cap_of: fn(u8) -> u8) {
        self.gate.receiver = Some(receiver);
        self.gate.cap_of = Some(cap_of);
    }

    /// What the layer has seen so far. Read it at the end of a run.
    #[must_use]
    pub fn counters(&self) -> Counters {
        self.gate.stats
    }

    /// Whether the layer is pinned to grid mode.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.pinned_shown
    }

    /// Record the abort of an authenticated sealed frame, such as a body that
    /// fails to open. More than [`PIN_AFTER_ABORTS`] in [`PIN_WINDOW_MS`] pins
    /// the layer. Track 0 and unverified frames never call this.
    pub fn sealed_abort(&mut self, now_ms: u64) -> Vec<Item> {
        let was = self.gate.pinned;
        self.gate.count_abort(now_ms);
        self.gate.pin_pending = false;
        if self.gate.pinned && !was {
            self.pinned_shown = true;
            self.gate.stats.pinned_notices += 1;
            vec![Item::Pinned]
        } else {
            Vec::new()
        }
    }

    /// The terminal opens the container for a new command and resets the
    /// producer's budget. Frame output never calls this.
    pub fn open_container(&mut self) {
        self.container_open = true;
        self.frames = 0;
        self.bytes = 0;
        self.degraded = false;
    }

    /// The terminal closes the container.
    pub fn close_container(&mut self) {
        self.container_open = false;
    }

    /// Process one read of PTY bytes.
    pub fn feed(&mut self, data: &[u8], ctx: Context) -> Vec<Item> {
        let mut events = Vec::new();
        self.gate.now_ms = ctx.now_ms;
        self.detector
            .feed_checked(data, &mut events, &mut self.gate);
        self.items(events, ctx)
    }

    /// Process the end of the stream.
    pub fn finish(&mut self) -> Vec<Item> {
        let mut events = Vec::new();
        self.detector.finish(&mut events);
        self.items(events, Context::default())
    }

    fn items(&mut self, events: Vec<Event>, ctx: Context) -> Vec<Item> {
        let mut items = Vec::new();
        for event in events {
            match event {
                Event::Output(bytes) => items.push(Item::Output(bytes)),
                Event::Frame { kind, payload } => {
                    self.gate.stats.frames_seen += 1;
                    self.gate.stats.frames_accepted += 1;
                    self.frame(kind, &payload, ctx, &mut items);
                }
                Event::Rejected(bytes) => {
                    self.gate.stats.frames_seen += 1;
                    self.gate.stats.frames_refused += 1;
                    if !self.gate.refusal_noted {
                        self.gate.stats.refused_framing += 1;
                    }
                    self.gate.refusal_noted = false;
                    // A refused candidate is not a frame: the detector has
                    // already released its bytes as ordinary output. Only the
                    // budget is charged.
                    self.charge(bytes.len(), &mut items);
                    if self.gate.pin_pending {
                        self.gate.pin_pending = false;
                        self.pinned_shown = true;
                        items.push(Item::Pinned);
                    }
                }
                Event::Truncated(bytes) => {
                    self.gate.stats.truncated += 1;
                    self.charge(bytes.len(), &mut items);
                    items.push(text(&bytes));
                }
            }
        }
        self.tally(&items);
        items
    }

    /// Count the items a read produced.
    fn tally(&mut self, items: &[Item]) {
        let s = &mut self.gate.stats;
        for item in items {
            match item {
                Item::Block(_) => s.track0_blocks += 1,
                Item::Background(_) => s.background_blocks += 1,
                Item::Text(_) => s.text_items += 1,
                Item::Degraded => s.degraded_notices += 1,
                Item::Pinned => s.pinned_notices += 1,
                Item::Sealed { .. } => s.sealed_opened += 1,
                Item::Output(_) => {}
            }
        }
    }

    /// Open the body of a frame whose header verified. A body that does not
    /// open is the abort of an authenticated frame: it is shown once as a
    /// failed frame and counts toward the pin.
    fn open_sealed(
        &mut self,
        header: Header,
        payload: &[u8],
        raw: &[u8],
        ctx: Context,
        items: &mut Vec<Item>,
    ) {
        let body = payload.get(HEADER_LEN..).unwrap_or_default();
        let opened = match (self.gate.receiver.as_ref(), self.gate.cap_of) {
            (Some(receiver), Some(cap_of)) => {
                receiver.open_body(&header, cap_of(header.kind), body).ok()
            }
            _ => None,
        };
        if let Some(plain) = opened {
            items.push(Item::Sealed {
                header,
                body: plain,
            });
            return;
        }
        items.push(text(raw));
        self.gate.stats.body_aead_failures += 1;
        self.gate.count_abort(ctx.now_ms);
        if self.gate.pin_pending {
            self.gate.pin_pending = false;
            self.pinned_shown = true;
            self.gate.verified.clear();
            items.push(Item::Pinned);
        }
    }

    /// Count a frame against the budget. The first time it is spent, whatever
    /// the cause, the notice is pushed. Returns whether the budget is spent.
    fn charge(&mut self, len: usize, items: &mut Vec<Item>) -> bool {
        self.frames = self.frames.saturating_add(1);
        self.bytes = self.bytes.saturating_add(len).saturating_add(1);
        let over = self.frames > self.config.budget.max_frames
            || self.bytes > self.config.budget.max_bytes;
        if over && !self.degraded {
            self.degraded = true;
            items.push(Item::Degraded);
        }
        self.degraded
    }

    fn frame(&mut self, kind: u8, payload: &[u8], ctx: Context, items: &mut Vec<Item>) {
        let mut raw = Vec::with_capacity(payload.len() + 1);
        raw.push(kind);
        raw.extend_from_slice(payload);
        if self.pinned_shown {
            items.push(text(&raw));
            return;
        }
        if kind < lumen_track0::UNSEALED_MIN && self.gate.receiver.is_some() {
            // The detector only yields this frame after the gate accepted it.
            if let Some(header) = self.gate.verified.pop_front() {
                self.open_sealed(header, payload, &raw, ctx, items);
            } else {
                items.push(text(&raw));
            }
            return;
        }
        let spent = self.charge(payload.len(), items);
        if spent {
            items.push(text(&raw));
            return;
        }
        if ctx.alt_screen || ctx.tmux_or_screen {
            items.push(text(&raw));
            return;
        }
        match parse(kind, payload) {
            Ok(block) if self.container_open => items.push(Item::Block(block)),
            Ok(_) if self.config.background_as_text => items.push(text(&raw)),
            Ok(block) => items.push(Item::Background(block)),
            Err(_) => items.push(text(&raw)),
        }
    }
}

/// Escaped text for bytes that are not drawn as a block.
fn text(bytes: &[u8]) -> Item {
    Item::Text(escaped_text(bytes))
}

/// Every kind name [`item_summary`] can return.
pub const ITEM_KINDS: [&str; 7] = [
    "output",
    "block",
    "background",
    "text",
    "degraded",
    "sealed",
    "pinned",
];

/// The `(kind, len)` pair the run recorder logs for an item. It carries no
/// content: `len` is the byte length of an output run or sealed payload, the
/// character count of text, and the total string length of a block.
#[must_use]
pub fn item_summary(item: &Item) -> (&'static str, usize) {
    fn block_len(block: &Block) -> usize {
        match block {
            Block::Paragraph(text) | Block::Heading { text, .. } => text.len(),
            Block::Code { lang, text, .. } => lang.len() + text.len(),
        }
    }
    match item {
        Item::Output(bytes) => ("output", bytes.len()),
        Item::Block(block) => ("block", block_len(block)),
        Item::Background(block) => ("background", block_len(block)),
        Item::Text(text) => ("text", text.chars().count()),
        Item::Degraded => ("degraded", 0),
        Item::Sealed { body, .. } => ("sealed", body.len()),
        Item::Pinned => ("pinned", 0),
    }
}
