//! The list of blocks a stream makes, as the layer's pieces arrive.

use crate::grid::GridBlock;
use crate::guard::Guard;
use crate::layer::{Context, Layer, Notice, Piece};
use crate::strip::Strip;
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
    events: Vec<Event>,
}

impl Document {
    /// An empty document whose grid blocks are `columns` by `rows`.
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            columns,
            rows,
            blocks: Vec::new(),
            notices: Vec::new(),
            sealed: Vec::new(),
            cycle: 0,
            open_strips: BTreeSet::new(),
            guard: Guard::new(),
            events: Vec::new(),
        }
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
                    self.grid().feed(&kept);
                }
                Piece::Text(text) => {
                    let mut line = text.into_bytes();
                    line.extend_from_slice(b"\r\n");
                    self.grid().feed(&line);
                }
                Piece::Rich(content) => self.blocks.push(Block::Rich(RichBlock {
                    content,
                    background: false,
                    cycle: self.cycle,
                })),
                Piece::Background(content) => self.blocks.push(Block::Rich(RichBlock {
                    content,
                    background: true,
                    cycle: self.cycle,
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
            self.grid().feed(&rest);
        }
    }
}
