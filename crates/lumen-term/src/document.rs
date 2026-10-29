//! The list of blocks a stream makes, as the layer's pieces arrive.

use crate::grid::GridBlock;
use crate::layer::{Context, Layer, Notice, Piece};
use lumen_track0::Block as Track0Block;

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
                Piece::Bytes(bytes) => self.grid().feed(&bytes),
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
                Piece::Sealed { kind, len } => self.sealed.push((kind, len)),
            }
        }
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
    }
}
