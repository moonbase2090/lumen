//! Models for the multiplexer placement comparison (criteria section 7).
//!
//! This crate is a model, not a product: [`PaneTerminal`] is the outer
//! terminal's handling of the two sealed multiplexer frame types, and
//! [`Multiplexer`] is the sealed relay of Option A that produces them. It is
//! test-only: nothing shipped depends on it. The payloads follow the experiment
//! profile in `spec/v0/multiplexers.md` (Frame payloads), which is not yet
//! normative.
//!
//! The model draws nothing. It keeps a [`Draw`] log: every rectangle the
//! terminal would draw, which is what the draw-count metrics of 7.2 read.

#![forbid(unsafe_code)]

pub mod codec;
pub mod identity;
pub mod mux;
pub mod terminal;

pub use codec::{Layout, PaneContainer, Rect};
pub use identity::{Identity, PaneLabel};
pub use mux::Multiplexer;
pub use terminal::{Counters, Draw, Outcome, PaneTerminal};

/// The sealed frame type of a layout.
pub const TYPE_LAYOUT: u8 = 0x01;
/// The sealed frame type of a pane container.
pub const TYPE_PANE: u8 = 0x02;
/// The capability value both models seal and open multiplexer frames under:
/// the frame type table seals layout and pane container frames under none.
pub const MUX_CAP: u8 = lumen_sealed::types::CAP_NONE;
