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

use lumen_detector::{Detector, Event};
use lumen_track0::{Block, escaped_text, parse};

/// Per-read facts about the layer that change how Track 0 is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The alternate screen is active on this layer.
    pub alt_screen: bool,
    /// `TMUX` or `STY` is present in the layer's environment.
    pub tmux_or_screen: bool,
}

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
}

/// The Track 0 pipeline for one layer.
#[derive(Debug)]
pub struct Layer {
    detector: Detector,
    config: Config,
    container_open: bool,
    frames: usize,
    bytes: usize,
    degraded: bool,
}

impl Layer {
    /// A layer with no container open.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            detector: Detector::default(),
            config,
            container_open: false,
            frames: 0,
            bytes: 0,
            degraded: false,
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
        self.detector.feed(data, &mut events);
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
                Event::Frame { kind, payload } => self.frame(kind, &payload, ctx, &mut items),
                Event::Rejected(bytes) | Event::Truncated(bytes) => {
                    self.charge(bytes.len(), &mut items);
                    items.push(text(&bytes));
                }
            }
        }
        items
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
        let spent = self.charge(payload.len(), items);
        let mut raw = Vec::with_capacity(payload.len() + 1);
        raw.push(kind);
        raw.extend_from_slice(payload);
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
