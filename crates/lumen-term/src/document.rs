//! The list of blocks a stream makes, as the layer's pieces arrive.

use crate::grid::GridBlock;
use crate::guard::Guard;
use crate::images::{ImageHost, Pixels, Shown};
use crate::isolate::{Isolator, Note, Stats};
use crate::layer::{Context, Layer, Notice, Piece};
use crate::reply::ColourQuery;
use crate::strip::Strip;
use lumen_decoder::frames::{Data, Start};
use lumen_track0::Block as Track0Block;
use std::collections::BTreeSet;

/// A Track 0 block shown in the terminal's own container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichBlock {
    /// The inert content.
    pub content: Track0Block,
    /// True for the collapsed background strip (Arm B only).
    pub background: bool,
    /// The prompt cycle the block arrived in, so the strip can be one
    /// aggregate per layer per cycle.
    pub cycle: u32,
    /// For an image start: the decoded pixels once the decode has finished.
    /// Until then, and whenever the image is refused, the alt text is shown.
    pub image: Option<Pixels>,
}

/// Something the terminal has to handle that the document does not draw: a
/// notification sequence it removed, or a sealed frame that opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An OSC 9, 777 or 99 sequence in unsigned grid output, removed before
    /// the emulator saw it.
    Unsigned {
        /// The OSC number.
        number: u32,
        /// The prompt cycle it arrived in.
        cycle: u32,
    },
    /// A string in grid output reached its 4 KiB cap; the rest of it is
    /// discarded, so the terminal shows a "string truncated" notice.
    StringTruncated {
        /// The prompt cycle it arrived in.
        cycle: u32,
    },
    /// A sealed frame that verified and opened.
    Sealed {
        /// The sealed frame type.
        kind: u8,
        /// The decrypted payload.
        body: Vec<u8>,
    },
}

/// One block of the document.
pub enum Block {
    /// Terminal cells.
    Grid(Box<GridBlock>),
    /// Track 0 content.
    Rich(RichBlock),
}

/// The blocks of one stream, newest last.
pub struct Document {
    columns: usize,
    rows: usize,
    blocks: Vec<Block>,
    notices: Vec<Notice>,
    sealed: Vec<(u8, usize)>,
    cycle: u32,
    open_strips: BTreeSet<u32>,
    guard: Guard,
    isolator: Isolator,
    events: Vec<Event>,
    colour_queries: Vec<ColourQuery>,
    images: ImageHost,
    pending_title: Option<String>,
}

/// Most colour queries kept for the owner to answer. Later ones are not kept.
const MAX_PENDING_QUERIES: usize = 64;

impl Document {
    /// An empty document whose grid blocks are `columns` by `rows`.
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self::with_images(columns, rows, ImageHost::for_build(true))
    }

    /// An empty document with the given image path.
    #[must_use]
    pub fn with_images(columns: usize, rows: usize, images: ImageHost) -> Self {
        Self {
            columns,
            rows,
            blocks: Vec::new(),
            notices: Vec::new(),
            sealed: Vec::new(),
            cycle: 0,
            open_strips: BTreeSet::new(),
            guard: Guard::new(),
            isolator: Isolator::new(),
            events: Vec::new(),
            colour_queries: Vec::new(),
            images,
            pending_title: None,
        }
    }

    /// The local `images` setting: `false` always shows the alt text.
    pub fn set_images_setting(&mut self, on: bool) {
        self.images.set_setting(on);
    }

    /// The width, in columns, of every block.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// The height, in rows, of every grid block.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Resize every grid block, and the size new ones are made at.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        self.columns = columns;
        self.rows = rows;
        for block in &mut self.blocks {
            if let Block::Grid(g) = block {
                g.resize(columns, rows);
            }
        }
    }

    /// The blocks, oldest first.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The chrome notices raised so far.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// The sealed frames that opened, as (type, payload length).
    #[must_use]
    pub fn sealed(&self) -> &[(u8, usize)] {
        &self.sealed
    }

    /// The colour queries grid output has made since the last call, oldest
    /// first. The owner answers them with [`crate::reply::Replies`].
    pub fn take_colour_queries(&mut self) -> Vec<ColourQuery> {
        std::mem::take(&mut self.colour_queries)
    }

    /// What grid isolation has removed so far.
    #[must_use]
    pub fn isolation(&self) -> Stats {
        self.isolator.stats()
    }

    /// Give the emulator what grid isolation allows of `bytes`.
    fn isolate_and_feed(&mut self, bytes: &[u8]) {
        let mut allowed = Vec::with_capacity(bytes.len());
        let mut notes = Vec::new();
        self.isolator.filter(bytes, &mut allowed, &mut notes);
        for note in notes {
            match note {
                Note::StringTruncated => self
                    .events
                    .push(Event::StringTruncated { cycle: self.cycle }),
                Note::ColourQuery(query) => {
                    if self.colour_queries.len() < MAX_PENDING_QUERIES {
                        self.colour_queries.push(query);
                    }
                }
            }
        }
        if !allowed.is_empty() {
            let grid = self.grid();
            grid.feed(&allowed);
            if let Some(title) = grid.take_title() {
                self.pending_title = Some(title);
            }
        }
    }

    /// The newest window title the program set since the last call. `Some("")`
    /// is a reset. Only the program's own output sets it.
    pub fn take_title(&mut self) -> Option<String> {
        self.pending_title.take()
    }

    fn grid(&mut self) -> &mut GridBlock {
        if !matches!(self.blocks.last(), Some(Block::Grid(_))) {
            self.blocks.push(Block::Grid(Box::new(GridBlock::new(
                self.columns,
                self.rows,
            ))));
        }
        match self.blocks.last_mut() {
            Some(Block::Grid(g)) => g,
            _ => unreachable!("a grid block was just pushed"),
        }
    }

    /// Apply what the layer decided. Output and text go to the newest grid
    /// block, which a rich block ends: what comes after it starts a new one.
    pub fn apply(&mut self, pieces: Vec<Piece>) {
        for piece in pieces {
            match piece {
                Piece::Bytes(bytes) => {
                    // Notification sequences are removed before the emulator sees
                    // the bytes, on this path as on every other.
                    let mut kept = Vec::with_capacity(bytes.len());
                    let mut found = Vec::new();
                    self.guard.filter(&bytes, &mut kept, &mut found);
                    for f in found {
                        self.events.push(Event::Unsigned {
                            number: f.number,
                            cycle: self.cycle,
                        });
                    }
                    self.isolate_and_feed(&kept);
                }
                Piece::Text(text) => {
                    let mut line = text.into_bytes();
                    line.extend_from_slice(b"\r\n");
                    self.grid().feed(&line);
                }
                Piece::Rich(Track0Block::ImageStart {
                    id,
                    width,
                    height,
                    length,
                    alt,
                    format,
                }) => {
                    let (shown, abandoned) = self.images.start(Start {
                        id,
                        width,
                        height,
                        length,
                    });
                    debug_assert!(!matches!(shown, Shown::Image(_)));
                    let _ = abandoned;
                    self.blocks.push(Block::Rich(RichBlock {
                        content: Track0Block::ImageStart {
                            id,
                            format,
                            width,
                            height,
                            length,
                            alt,
                        },
                        background: false,
                        cycle: self.cycle,
                        image: None,
                    }));
                }
                Piece::Rich(Track0Block::ImageData { id, offset, data }) => {
                    if let Data::Rejected { .. } = self.images.data(id, offset, &data) {
                        // The frame did not fit: shown as escaped text.
                        let mut raw = vec![lumen_track0::KIND_IMAGE_DATA];
                        raw.extend_from_slice(&id.to_be_bytes());
                        raw.extend_from_slice(&offset.to_be_bytes());
                        raw.extend_from_slice(&data);
                        let mut line = lumen_track0::escaped_text(&raw).into_bytes();
                        line.extend_from_slice(b"\r\n");
                        self.grid().feed(&line);
                    }
                }
                // Image bytes never take a row in a strip.
                Piece::Background(Track0Block::ImageData { .. }) => {}
                Piece::Rich(content) => self.blocks.push(Block::Rich(RichBlock {
                    content,
                    background: false,
                    cycle: self.cycle,
                    image: None,
                })),
                Piece::Background(content) => self.blocks.push(Block::Rich(RichBlock {
                    content,
                    background: true,
                    cycle: self.cycle,
                    image: None,
                })),
                Piece::Notice(n) => self.notices.push(n),
                Piece::Sealed { kind, body } => {
                    self.sealed.push((kind, body.len()));
                    self.events.push(Event::Sealed { kind, body });
                }
            }
        }
    }

    /// Take the events raised since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// The background strips, one per prompt cycle that has background
    /// output, oldest first. A strip is collapsed unless the user opened it.
    #[must_use]
    pub fn strips(&self) -> Vec<Strip<'_>> {
        let mut out: Vec<Strip<'_>> = Vec::new();
        for block in &self.blocks {
            let Block::Rich(r) = block else { continue };
            if !r.background {
                continue;
            }
            match out.last_mut() {
                Some(s) if s.cycle == r.cycle => s.blocks.push(r),
                _ => out.push(Strip {
                    cycle: r.cycle,
                    blocks: vec![r],
                    open: self.open_strips.contains(&r.cycle),
                }),
            }
        }
        out
    }

    /// Takes the decodes that finished and puts their pixels in their blocks.
    /// Returns whether any block changed, so a window can draw again.
    pub fn poll_images(&mut self) -> bool {
        let mut changed = false;
        for update in self.images.poll() {
            let Shown::Image(pixels) = update.shown else {
                continue;
            };
            let target = self.blocks.iter_mut().rev().find_map(|b| match b {
                Block::Rich(r)
                    if !r.background
                        && matches!(&r.content,
                            Track0Block::ImageStart { id, .. } if *id == update.id) =>
                {
                    Some(r)
                }
                _ => None,
            });
            if let Some(r) = target {
                r.image = Some(pixels);
                changed = true;
            }
        }
        changed
    }

    /// The user opens the strip of `cycle`. This is the only way a strip
    /// opens: nothing the layer yields calls it. It returns false if that
    /// cycle has no strip.
    pub fn user_opens_strip(&mut self, cycle: u32) -> bool {
        if self.strips().iter().any(|s| s.cycle == cycle) {
            self.open_strips.insert(cycle);
            true
        } else {
            false
        }
    }

    /// The user closes the strip of `cycle`.
    pub fn user_closes_strip(&mut self, cycle: u32) {
        self.open_strips.remove(&cycle);
    }

    /// The prompt cycle blocks are arriving in. It starts at 0.
    #[must_use]
    pub fn cycle(&self) -> u32 {
        self.cycle
    }

    /// A new prompt cycle begins: the terminal opens the layer's container for
    /// the next command, and blocks that arrive from now on belong to the new
    /// cycle.
    pub fn begin_prompt_cycle(&mut self, layer: &mut dyn Layer) {
        // The command ended: an image still being received ends with its alt
        // text and its bytes are freed.
        let _ = self.images.end_command();
        self.cycle = self.cycle.saturating_add(1);
        layer.open_container();
    }

    /// Read from `layer`: feed it `bytes` with what the terminal knows about
    /// the layer, and apply what it yields.
    pub fn receive(&mut self, layer: &mut dyn Layer, bytes: &[u8], ctx: Context) {
        let pieces = layer.feed(bytes, ctx);
        self.apply(pieces);
    }

    /// The terminal refused an authenticated sealed frame: count it toward the
    /// layer's pin and apply any notice that raises.
    pub fn sealed_abort(&mut self, layer: &mut dyn Layer, now_ms: u64) {
        let pieces = layer.sealed_abort(now_ms);
        self.apply(pieces);
    }

    /// The stream ended.
    pub fn finish(&mut self, layer: &mut dyn Layer) {
        let pieces = layer.finish();
        self.apply(pieces);
        // Whatever the guard held back at the end of a stream goes on to the grid.
        let mut rest = Vec::new();
        self.guard.finish(&mut rest);
        if !rest.is_empty() {
            self.isolate_and_feed(&rest);
        }
        self.isolator.finish();
    }
}
