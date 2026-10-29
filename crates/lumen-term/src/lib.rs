//! The Lumen terminal, headless part: blocks, and the arm layer in front of them.
//!
//! A [`GridBlock`] is a region of terminal cells with its own emulator and,
//! when it runs a program, its own PTY sized to the block. A [`RichBlock`] is a
//! Track 0 block the arm layer let through. The [`Document`] turns the pieces
//! a [`Layer`] yields for the bytes of a stream into a list of blocks.
//!
//! [`raster`] paints blocks to an RGBA frame with one bundled face. Nothing
//! here opens a window. The layer is chosen by cargo
//! feature (`arm-a`, `arm-b`), so one source builds either arm, and the plain
//! [`Passthrough`] layer is for a stream with no arm in front of it.

#![forbid(unsafe_code)]

pub mod document;
pub mod grid;
pub mod layer;
pub mod layout;
mod pty;
pub mod raster;
pub mod session;
pub mod strip;

pub use document::{Block, Document, RichBlock};
pub use grid::GridBlock;
pub use layer::{Context, Layer, Notice, Passthrough, Piece};
pub use layout::{Layout, Rect, StripPlacement, layout};
pub use pty::SpawnError;
pub use raster::{Frame, Renderer, Theme, Window};
pub use session::Session;
pub use strip::Strip;
