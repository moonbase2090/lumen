//! The Lumen terminal, headless part: blocks, and the arm layer in front of them.
//!
//! A [`GridBlock`] is a region of terminal cells with its own emulator and,
//! when it runs a program, its own PTY sized to the block. A [`RichBlock`] is a
//! Track 0 block the arm layer let through. The [`Document`] turns the pieces
//! a [`Layer`] yields for the bytes of a stream into a list of blocks.
//!
//! [`raster`] paints blocks to an RGBA frame with one bundled face. Nothing
//! here opens a window. The terminal is built on Arm B (Track 0 rich on every
//! layer, with the sealed upgrade): [`Layer`] is implemented for its layer, and
//! the plain [`Passthrough`] layer is for a stream with no arm in front of it.
//! Arm A is kept as the evidence record in its own crate and is not used here.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod document;
pub mod grid;
pub mod guard;
#[cfg(feature = "window")]
pub mod host;
pub mod isolate;
pub mod keys;
pub mod layer;
pub mod layout;
pub mod notify;
pub mod os_backend;
pub mod policy;
mod pty;
pub mod raster;
pub mod session;
pub mod strip;

pub use app::App;
pub use document::{Block, Document, Event, RichBlock};
pub use grid::GridBlock;
pub use keys::{Key, Mods};
pub use layer::{Context, Layer, Notice, Passthrough, Piece};
pub use layout::{
    GUTTER_COLUMNS, HEADER_ROWS, Layout, Rect, StripPlacement, layout, notification_area,
};
pub use pty::SpawnError;
pub use raster::{Frame, Renderer, Theme, Window, wrap_text};
pub use session::Session;
pub use strip::Strip;
