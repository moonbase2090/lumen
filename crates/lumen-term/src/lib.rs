//! The Lumen terminal, headless part: blocks, and the arm layer in front of them.
//!
//! A [`GridBlock`] is a region of terminal cells with its own emulator and,
//! when it runs a program, its own PTY sized to the block. A [`RichBlock`] is a
//! Track 0 block the arm layer let through. The [`Document`] turns the pieces
//! a [`Layer`] yields for the bytes of a stream into a list of blocks.
//!
//! Nothing here opens a window or draws a pixel. The layer is chosen by cargo
//! feature (`arm-a`, `arm-b`), so one source builds either arm, and the plain
//! [`Passthrough`] layer is for a stream with no arm in front of it.

#![forbid(unsafe_code)]

pub mod document;
pub mod grid;
pub mod layer;

pub use document::{Block, Document, RichBlock};
pub use grid::GridBlock;
pub use layer::{Context, Layer, Notice, Passthrough, Piece};
