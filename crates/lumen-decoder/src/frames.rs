//! Receiving an image from its frames.
//!
//! An image arrives as one start frame and then in-order data frames. The
//! [`Receiver`] keeps at most one image being received per producer, checks
//! every data frame against it, buffers nothing while images are locked, and
//! hands a complete image to the [`ImageManager`]. The buffer grows only with
//! bytes that have arrived. A declared length is never used as an allocation
//! size.
//!
//! Check order for a start, after the frame has parsed (a frame that does not
//! parse never reaches here, and so does not end an image already being
//! received):
//!
//! 1. Locked, SSH, or already degraded: show the alt text and drop this id's
//!    data frames. An incomplete image of this producer ends.
//! 2. Encoded length outside 1..=16,777,216: same, with no degrade.
//! 3. A size hint of zero, over 65,535 on a side, or over 16,000,000 pixels:
//!    same.
//! 4. An id that is in use, including the image being received: same. The new
//!    start does not begin another image.
//! 5. Queued encoded bytes plus this length over 32 MiB: same, and the
//!    producer is degraded for the rest of the command.
//! 6. Otherwise the image is received. A start always ends an incomplete image
//!    of the same producer first.
//!
//! A data frame is kept only for the image being received, with the same id,
//! an offset equal to the bytes so far, 1 to 262,144 bytes, and a total that
//! does not pass the declared length. Any other data frame ends that image.
//! Data frames of an id that was refused are dropped, and a data frame for a
//! different id does not stop that dropping.

use std::collections::HashMap;

use crate::images::{AltReason, ImageId, ImageManager, Producer, Submit, TextureSink};
use crate::limits;
use crate::protocol::Format;

/// Most encoded bytes of one image (16 MiB).
pub const MAX_IMAGE_BYTES: u32 = 16_777_216;
/// Most encoded bytes one producer holds at once, queued and being received.
pub const MAX_HELD_BYTES: u64 = 32 * 1024 * 1024;
/// Most encoded bytes one data frame carries.
pub const MAX_CHUNK: usize = 262_144;

/// The fields of an image start frame that the receiver checks.
///
/// The format byte is not one of them. It is a hint, and the format is taken
/// from the encoded bytes once they are complete.
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
    /// An earlier incomplete image of the producer that this start ended.
    pub abandoned: Option<ImageId>,
}

/// Where a start frame left its image.
#[derive(Debug, PartialEq, Eq)]
pub enum Placement {
    /// The data frames are expected.
    Receiving,
    /// The alt text is shown. Data frames for this id are dropped.
    Alt(AltReason),
}

/// What a data frame did.
#[derive(Debug, PartialEq, Eq)]
pub enum Data {
    /// Kept. More is expected.
    Buffered,
    /// The image is complete and was submitted.
    Submitted(ImageId, Submit),
    /// The frame belongs to an image that was refused at its start. It was
    /// not kept.
    Dropped,
    /// The frame did not fit. The incomplete image, if any, ended.
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
    /// The id whose start was refused. Its data frames are dropped.
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
            .map_or(0, |p| u64::try_from(p.bytes.len()).unwrap_or(u64::MAX))
    }

    /// Handles an image start frame.
    #[must_use]
    pub fn start<T: TextureSink>(
        &mut self,
        manager: &mut ImageManager<T>,
        producer: Producer,
        start: Start,
    ) -> Started {
        let state = self.producers.entry(producer).or_default();
        // The id being received is in use. Record it before the start ends it.
        let incumbent = state.partial.as_ref().map(|p| p.id);
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
        if manager.has_id(producer, start.id) || incumbent == Some(start.id) {
            return refuse(state, AltReason::DuplicateId);
        }
        let held = manager
            .held_bytes(producer)
            .saturating_add(u64::from(start.length));
        if held > MAX_HELD_BYTES {
            manager.degrade(producer);
            return refuse(state, AltReason::HeldBytes);
        }
        let Some(length) = usize::try_from(start.length).ok() else {
            return refuse(state, AltReason::BadLength);
        };
        state.partial = Some(Partial {
            id: start.id,
            hint: (start.width, start.height),
            length,
            bytes: Vec::new(),
        });
        Started {
            placement: Placement::Receiving,
            abandoned,
        }
    }

    /// Handles an image data frame.
    ///
    /// `bytes` must already be the 1 to [`MAX_CHUNK`] bytes of a parsed data
    /// frame. A slice outside that range is not a data frame: it ends the
    /// incomplete image, the same as a gap or an overrun, so it cannot be stored.
    #[must_use]
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
            // Dropping is only for the refused id. A frame for any other id
            // is escaped text and does not stop the dropping.
            if state.skipping == Some(id) {
                return Data::Dropped;
            }
            return Data::Rejected { abandoned: None };
        }
        let fits = state
            .partial
            .as_ref()
            .is_some_and(|p| chunk_fits(p, id, offset, bytes));
        if !fits {
            let abandoned = state.partial.take().map(|p| p.id);
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
        // The format comes from the bytes, never from the producer's hint.
        let Some(format) = Format::sniff(&done.bytes) else {
            return Data::Submitted(done.id, Submit::AltText(AltReason::Refused));
        };
        let submit = manager.submit(producer, done.id, format, done.hint, done.bytes);
        Data::Submitted(done.id, submit)
    }

    /// The command or the layer of `producer` ended, or its stream was reset.
    /// An incomplete image ends and its bytes are freed. Degradation lifts
    /// with the command. Shown and queued ids stay in use.
    #[must_use]
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

/// Whether `bytes` is the next chunk of `partial`.
fn chunk_fits(partial: &Partial, id: ImageId, offset: u32, bytes: &[u8]) -> bool {
    if partial.id != id || bytes.is_empty() || bytes.len() > MAX_CHUNK {
        return false;
    }
    let Ok(at) = u64::try_from(partial.bytes.len()) else {
        return false;
    };
    if u64::from(offset) != at {
        return false;
    }
    partial
        .bytes
        .len()
        .checked_add(bytes.len())
        .is_some_and(|end| end <= partial.length)
}
