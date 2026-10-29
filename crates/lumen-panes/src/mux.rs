//! Option A: a multiplexer that relays sealed layout and pane container frames.

use crate::codec::{Layout, PaneContainer, Rect};
use crate::{MUX_CAP, TYPE_LAYOUT, TYPE_PANE};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Sender};

/// A multiplexer holding the sealing keys of its layer.
///
/// In the real design the keys stay in the client and never reach the server;
/// this model is one process and does not measure that (key custody is a
/// separate metric).
#[derive(Debug)]
pub struct Multiplexer {
    sender: Sender,
    next_pane: u32,
    generation: u32,
}

impl Multiplexer {
    /// A multiplexer for a layer, sealing under keys derived from `root`.
    #[must_use]
    pub fn new(root: &LayerRoot, layer: LayerId) -> Self {
        let keys = DirectionKeys::derive_multiplexer(root, layer, Direction::ToTerminal);
        Self {
            sender: Sender::new(keys, layer, Direction::ToTerminal, 1),
            next_pane: 1,
            generation: 0,
        }
    }

    /// A new pane id. Ids come from a counter that only increases and are never
    /// reused.
    pub fn new_pane(&mut self) -> u32 {
        let id = self.next_pane;
        self.next_pane += 1;
        id
    }

    /// The generation of the latest layout this multiplexer sent.
    #[must_use]
    pub fn generation(&self) -> u32 {
        self.generation
    }

    fn seal(&mut self, kind: u8, plain: &[u8]) -> Vec<u8> {
        match self.sender.seal(kind, MUX_CAP, plain) {
            Ok(frame) => frame,
            Err(e) => unreachable!("sealing a short payload succeeds: {e:?}"),
        }
    }

    /// A layout frame for the next generation, listing the visible panes.
    pub fn layout(&mut self, panes: &[(u32, Rect)]) -> Vec<u8> {
        self.generation += 1;
        self.layout_at(self.generation, panes)
    }

    /// A layout frame at an explicit generation (to send a stale or early one).
    pub fn layout_at(&mut self, generation: u32, panes: &[(u32, Rect)]) -> Vec<u8> {
        let payload = Layout {
            generation,
            panes: panes.to_vec(),
        }
        .encode();
        self.seal(TYPE_LAYOUT, &payload)
    }

    /// A pane container frame at the current generation.
    pub fn container(&mut self, pane: u32, rect: Rect, kind: u8, content: &[u8]) -> Vec<u8> {
        self.container_at(self.generation, pane, rect, kind, content)
    }

    /// A pane container frame at an explicit generation.
    pub fn container_at(
        &mut self,
        generation: u32,
        pane: u32,
        rect: Rect,
        kind: u8,
        content: &[u8],
    ) -> Vec<u8> {
        let payload = PaneContainer {
            pane,
            generation,
            rect,
            kind,
            content: content.to_vec(),
        }
        .encode();
        self.seal(TYPE_PANE, &payload)
    }

    /// A sealed frame of any type with a raw payload, for the cases a correct
    /// multiplexer never sends.
    pub fn raw(&mut self, kind: u8, payload: &[u8]) -> Vec<u8> {
        self.seal(kind, payload)
    }
}
