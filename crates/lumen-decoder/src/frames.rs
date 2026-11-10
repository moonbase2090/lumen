//! Receiving an image from its frames: the rules of [images.frames] in the spec.
//!
//! An image arrives as one start frame and then in-order data frames. The
//! [`Receiver`] keeps at most one image being received per producer, checks every
//! data frame against it, refuses to buffer anything while images are locked,
//! and hands the complete bytes to the [`ImageManager`]. It never trusts a
//! declared length for an allocation: the buffer grows only with bytes that have
//! arrived, and those are bounded by the length, which is itself bounded.

use std::collections::HashMap;

use crate::images::{AltReason, ImageId, ImageManager, Producer, Submit, TextureSink};
use crate::limits;
use crate::protocol::Format;

/// Most encoded bytes of one image (16 MiB).
pub const MAX_IMAGE_BYTES: u32 = 16 * 1024 * 1024;
/// Most encoded bytes one producer holds at once, in the image being received and
/// in images waiting for a decode (32 MiB).
pub const MAX_HELD_BYTES: u64 = 32 * 1024 * 1024;
/// Most encoded bytes one data frame carries.
pub const MAX_CHUNK: usize = 262_144;

/// The fields of an image start frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Start {
    /// The image's id, scoped to its producer.
    pub id: ImageId,
    /// The declared width, a hint.
    pub width: u32,
    /// The declared height, a hint.
    pub height: u32,
    /// The total encoded bytes the data frames will carry.
    pub length: u32,
}

/// What a start frame did.
#[derive(Debug, PartialEq, Eq)]
pub struct Started {
    /// Whether the image is being received, or why the alt text is shown.
    pub placement: Placement,
    /// An earlier incomplete image of the producer that this start ended: its
    /// alt text is shown.
    pub abandoned: Option<ImageId>,
}

/// Where a start frame left its image.
#[derive(Debug, PartialEq, Eq)]
pub enum Placement {
    /// The data frames are expected.
    Receiving,
    /// The alt text is shown; the data frames are dropped as they arrive.
    Alt(AltReason),
}

/// What a data frame did.
#[derive(Debug, PartialEq, Eq)]
pub enum Data {
    /// Kept; more is expected.
    Buffered,
    /// The image is complete and was submitted; `Submit` says what became of it.
    Submitted(ImageId, Submit),
    /// The frame belongs to an image shown as alt text (locked, refused at its
    /// start): dropped without being kept.
    Dropped,
    /// The frame did not fit: the incomplete image, if any, ended with its alt
    /// text. The frame is shown as escaped text.
    Rejected {
        /// The incomplete image this ended, if there was one.
        abandoned: Option<ImageId>,
    },
}

struct Partial {
    id: ImageId,
    hint: (u32, u32),
    length: usize,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    partial: Option<Partial>,
    /// The image whose start was refused: its data frames are dropped.
    skipping: Option<ImageId>,
}

/// See the module documentation.
#[derive(Default)]
pub struct Receiver {
    producers: HashMap<Producer, State>,
}

impl Receiver {
    /// A receiver with nothing in progress.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Encoded bytes being received for `producer`.
    #[must_use]
    pub fn receiving_bytes(&self, producer: Producer) -> u64 {
        self.producers
            .get(&producer)
            .and_then(|s| s.partial.as_ref())
            .map_or(0, |p| p.bytes.len() as u64)
    }

    /// Handles an image start frame.
    pub fn start<T: TextureSink>(
        &mut self,
        manager: &mut ImageManager<T>,
        producer: Producer,
        start: Start,
    ) -> Started {
        let state = self.producers.entry(producer).or_default();
        let abandoned = state.partial.take().map(|p| p.id);
        state.skipping = None;
        let refuse = |state: &mut State, reason: AltReason| {
            state.skipping = Some(start.id);
            Started {
                placement: Placement::Alt(reason),
                abandoned,
            }
        };
        if let Some(reason) = manager.refusal(producer) {
            return refuse(state, reason);
        }
        if start.length == 0 || start.length > MAX_IMAGE_BYTES {
            return refuse(state, AltReason::BadLength);
        }
        if limits::rgba_bytes(u64::from(start.width), u64::from(start.height)).is_none() {
            return refuse(state, AltReason::InvalidHint);
        }
        if manager.has_id(producer, start.id) {
            return refuse(state, AltReason::DuplicateId);
        }
        if manager.held_bytes(producer) + u64::from(start.length) > MAX_HELD_BYTES {
            manager.degrade(producer);
            return refuse(state, AltReason::HeldBytes);
        }
        state.partial = Some(Partial {
            id: start.id,
            hint: (start.width, start.height),
            length: start.length as usize,
            bytes: Vec::new(),
        });
        Started {
            placement: Placement::Receiving,
            abandoned,
        }
    }

    /// Handles an image data frame.
    pub fn data<T: TextureSink>(
        &mut self,
        manager: &mut ImageManager<T>,
        producer: Producer,
        id: ImageId,
        offset: u32,
        bytes: &[u8],
    ) -> Data {
        let state = self.producers.entry(producer).or_default();
        if state.partial.is_none() {
            // Only the refused id is dropped. A frame for any other id is
            // rejected and does not stop the dropping.
            if state.skipping == Some(id) {
                return Data::Dropped;
            }
            return Data::Rejected { abandoned: None };
        }
        let fits = state.partial.as_ref().is_some_and(|p| {
            p.id == id
                && u64::from(offset) == p.bytes.len() as u64
                && !bytes.is_empty()
                && bytes.len() <= MAX_CHUNK
                && p.bytes.len() + bytes.len() <= p.length
        });
        if !fits {
            let abandoned = state.partial.take().map(|p| p.id);
            state.skipping = None;
            return Data::Rejected { abandoned };
        }
        let Some(partial) = state.partial.as_mut() else {
            return Data::Rejected { abandoned: None };
        };
        partial.bytes.extend_from_slice(bytes);
        if partial.bytes.len() < partial.length {
            return Data::Buffered;
        }
        let Some(done) = state.partial.take() else {
            return Data::Rejected { abandoned: None };
        };
        // The terminal sets the format from the bytes, not from the producer.
        let Some(format) = Format::sniff(&done.bytes) else {
            return Data::Submitted(done.id, Submit::AltText(AltReason::Refused));
        };
        let submit = manager.submit(producer, done.id, format, done.hint, done.bytes);
        Data::Submitted(id, submit)
    }

    /// The command or the layer of `producer` ended, or its stream was reset:
    /// an incomplete image ends with its alt text and its bytes are freed.
    pub fn end<T: TextureSink>(
        &mut self,
        manager: &mut ImageManager<T>,
        producer: Producer,
    ) -> Option<ImageId> {
        manager.end_command(producer);
        self.producers
            .remove(&producer)
            .and_then(|s| s.partial)
            .map(|p| p.id)
    }
}
