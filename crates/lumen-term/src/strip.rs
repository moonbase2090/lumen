//! The background output strip.
//!
//! Track 0 can arrive while no terminal-opened container is open. Arm B puts
//! it in one collapsed strip, labelled `background output (N)`, which the
//! terminal draws. There is one strip per layer (a [`Document`] is one layer;
//! under a Lumen-aware multiplexer each pane has its own) per prompt cycle. It
//! opens only on a user action and uses the producer's single unsigned budget,
//! which the arm layer enforces: when the budget is spent, or the policy sets
//! `background_track0 = text`, further orphan output reaches the document as
//! escaped text and never as a strip entry.
//!
//! [`Document`]: crate::Document

use crate::document::RichBlock;

/// One strip, as the terminal draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip<'a> {
    /// The prompt cycle it belongs to.
    pub cycle: u32,
    /// The background blocks in it, oldest first.
    pub blocks: Vec<&'a RichBlock>,
    /// The user opened it.
    pub open: bool,
}

impl Strip<'_> {
    /// The number in the label.
    #[must_use]
    pub fn count(&self) -> usize {
        self.blocks.len()
    }

    /// The label the terminal draws on the collapsed strip. It holds only a
    /// count: nothing the producer wrote is in it.
    #[must_use]
    pub fn label(&self) -> String {
        format!("background output ({})", self.count())
    }
}
