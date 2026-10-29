//! The outer terminal's handling of the layout and pane container frames.

use crate::codec::{Layout, Malformed, PaneContainer, Rect};
use crate::{MUX_CAP, TYPE_LAYOUT, TYPE_PANE};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver};
use lumen_track0::Block;

/// One container the terminal would draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draw {
    /// The pane.
    pub pane: u32,
    /// The generation it was drawn under.
    pub generation: u32,
    /// The rectangle drawn: the container's rectangle clipped to its pane's
    /// rectangle in the current layout.
    pub rect: Rect,
    /// The content.
    pub block: Block,
}

/// What the terminal did with one sealed frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A layout took effect.
    LayoutAccepted,
    /// A layout with an equal or lower generation: no effect, counted.
    StaleLayout,
    /// A container was drawn.
    Drawn,
    /// A container with a lower generation: no effect, counted.
    StaleContainer,
    /// A container with a higher generation, or with no layout yet: no effect,
    /// counted.
    EarlyContainer,
    /// A container for a pane the layout does not list, or with a rectangle
    /// that is not the layout's: dropped, counted.
    DroppedContainer,
    /// A container whose content does not parse: dropped, counted.
    UnparsedContent,
    /// A layout or container that is malformed: a failed frame.
    Malformed(Malformed),
    /// A frame that did not verify or open: a failed frame.
    Refused,
    /// A sealed type a multiplexer's keys do not cover: a failed frame, and the
    /// layer is desynchronised.
    OtherType,
    /// The layer is desynchronised: panes are text and nothing is drawn.
    Ignored,
}

/// Counts of what happened, for the metrics of 7.2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Layouts that took effect.
    pub layouts_accepted: u32,
    /// Stale layouts.
    pub layouts_stale: u32,
    /// Containers drawn.
    pub containers_drawn: u32,
    /// Stale containers.
    pub containers_stale: u32,
    /// Early containers.
    pub containers_early: u32,
    /// Containers for unlisted panes or with a different rectangle.
    pub containers_dropped: u32,
    /// Containers whose content did not parse.
    pub containers_unparsed: u32,
    /// Malformed layouts and containers.
    pub malformed: u32,
    /// Frames that did not verify or open.
    pub refused: u32,
    /// Frames of a type the keys do not cover.
    pub other_type: u32,
    /// Frames counted toward the pin: malformed, refused and other-type frames.
    /// Stale and early frames are races and are not here.
    pub failed_frames: u32,
    /// Times the layout was discarded because the area changed.
    pub resets: u32,
}

/// The outer terminal's multiplexer layer.
#[derive(Debug)]
pub struct PaneTerminal {
    receiver: Receiver,
    cols: u32,
    rows: u32,
    layout: Option<Layout>,
    highest: Option<u32>,
    desync: bool,
    counters: Counters,
    draws: Vec<Draw>,
}

fn intersect(a: &Rect, b: &Rect) -> Option<Rect> {
    let x0 = i64::from(a.x).max(i64::from(b.x));
    let y0 = i64::from(a.y).max(i64::from(b.y));
    let x1 = (i64::from(a.x) + i64::from(a.w)).min(i64::from(b.x) + i64::from(b.w));
    let y1 = (i64::from(a.y) + i64::from(a.h)).min(i64::from(b.y) + i64::from(b.h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(Rect {
        x: i32::try_from(x0).ok()?,
        y: i32::try_from(y0).ok()?,
        w: u32::try_from(x1 - x0).ok()?,
        h: u32::try_from(y1 - y0).ok()?,
    })
}

impl PaneTerminal {
    /// A new multiplexer layer of `cols` by `rows` cells, holding the keys
    /// derived from `root`. It has no layout and no generation.
    #[must_use]
    pub fn new(root: &LayerRoot, layer: LayerId, cols: u32, rows: u32) -> Self {
        let keys = DirectionKeys::derive(root, layer, Direction::ToTerminal);
        Self {
            receiver: Receiver::new(keys, layer, Direction::ToTerminal),
            cols,
            rows,
            layout: None,
            highest: None,
            desync: false,
            counters: Counters::default(),
            draws: Vec::new(),
        }
    }

    /// The counters so far.
    #[must_use]
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Everything the terminal would have drawn, in order.
    #[must_use]
    pub fn draws(&self) -> &[Draw] {
        &self.draws
    }

    /// The panes of the current layout, if there is one.
    #[must_use]
    pub fn visible(&self) -> Vec<u32> {
        self.layout
            .as_ref()
            .map_or_else(Vec::new, |l| l.panes.iter().map(|(id, _)| *id).collect())
    }

    /// Whether the layer is desynchronised (panes are text).
    #[must_use]
    pub fn is_desynchronised(&self) -> bool {
        self.desync
    }

    /// The area changed size: the current layout is discarded. No pane is drawn
    /// until a layout with a greater generation than any accepted arrives.
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
        self.layout = None;
        self.counters.resets += 1;
    }

    fn fail(&mut self, counter: fn(&mut Counters) -> &mut u32) {
        *counter(&mut self.counters) += 1;
        self.counters.failed_frames += 1;
    }

    /// Handle one sealed frame (header and sealed payload).
    pub fn receive(&mut self, frame: &[u8]) -> Outcome {
        if self.desync {
            return Outcome::Ignored;
        }
        let Ok((header, plain)) = self.receiver.open(MUX_CAP, frame) else {
            self.fail(|c| &mut c.refused);
            return Outcome::Refused;
        };
        match header.kind {
            TYPE_LAYOUT => self.layout_frame(&plain),
            TYPE_PANE => self.container_frame(&plain),
            _ => {
                self.desync = true;
                self.layout = None;
                self.fail(|c| &mut c.other_type);
                Outcome::OtherType
            }
        }
    }

    fn layout_frame(&mut self, plain: &[u8]) -> Outcome {
        let layout = match Layout::decode(plain, self.cols, self.rows) {
            Ok(l) => l,
            Err(m) => {
                self.fail(|c| &mut c.malformed);
                return Outcome::Malformed(m);
            }
        };
        if self.highest.is_some_and(|h| layout.generation <= h) {
            self.counters.layouts_stale += 1;
            return Outcome::StaleLayout;
        }
        self.highest = Some(layout.generation);
        self.layout = Some(layout);
        self.counters.layouts_accepted += 1;
        Outcome::LayoutAccepted
    }

    fn container_frame(&mut self, plain: &[u8]) -> Outcome {
        let c = match PaneContainer::decode(plain, self.cols, self.rows) {
            Ok(c) => c,
            Err(m) => {
                self.fail(|c| &mut c.malformed);
                return Outcome::Malformed(m);
            }
        };
        let Some(layout) = &self.layout else {
            self.counters.containers_early += 1;
            return Outcome::EarlyContainer;
        };
        if c.generation < layout.generation {
            self.counters.containers_stale += 1;
            return Outcome::StaleContainer;
        }
        if c.generation > layout.generation {
            self.counters.containers_early += 1;
            return Outcome::EarlyContainer;
        }
        let Some((_, pane_rect)) = layout.panes.iter().find(|(id, _)| *id == c.pane) else {
            self.counters.containers_dropped += 1;
            return Outcome::DroppedContainer;
        };
        if *pane_rect != c.rect {
            self.counters.containers_dropped += 1;
            return Outcome::DroppedContainer;
        }
        let Ok(block) = lumen_track0::parse(c.kind, &c.content) else {
            self.counters.containers_unparsed += 1;
            return Outcome::UnparsedContent;
        };
        let Some(rect) = intersect(&c.rect, pane_rect) else {
            self.counters.containers_dropped += 1;
            return Outcome::DroppedContainer;
        };
        self.draws.push(Draw {
            pane: c.pane,
            generation: c.generation,
            rect,
            block,
        });
        self.counters.containers_drawn += 1;
        Outcome::Drawn
    }
}
