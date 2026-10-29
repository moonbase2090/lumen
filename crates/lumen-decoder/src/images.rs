//! The terminal's image manager: what the spec fixes about images once their
//! bytes have arrived, whatever frame carried them ([images], [trust.must2] and
//! [trust.must4] in the spec).
//!
//! - **Unlock.** Nothing is decoded unless [`crate::unlock::images_enabled`] says
//!   so for the build; otherwise the alt text is shown.
//! - **Possibly remote.** An image in Track 0 on an SSH session is never decoded.
//! - **Reserve first.** Each image reserves `width * height * 4` bytes of the
//!   decoded-image budget (256 MiB per session, 25% of that for an unsigned
//!   producer) when it is submitted, before any decoder runs; queued images
//!   count. The declared size is only a hint, and the decoded size replaces the
//!   reservation once known.
//! - **Concurrency.** At most 1 decode in flight per producer and 2 across the
//!   terminal; a producer can have 8 images waiting; past that, alt text.
//! - **Failure.** A decoder crash (including a reply that fails validation) puts
//!   that producer on alt text for the rest of the command, and so does going
//!   over budget (the producer degrades for the rest of the command, [trust.must4]).
//!   A decoder that merely declines (corrupt image, over a limit) does not.
//! - **Upload.** A texture upload is size-checked against the 512 MiB texture
//!   budget first; the sink is not called when it would not fit.
//!
//! What this does not do: parse an image out of a frame. The spec defines no image
//! frame yet, so a caller hands over the already-reassembled bytes with the
//! producer, id, format and the declared size.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::limits;
use crate::protocol::{Format, Status};
use crate::runner::{Crash, Outcome, Runner};
use crate::unlock::{GateRecord, Identities, images_enabled};

/// Who an image came from: a layer, by the id the terminal assigned it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Producer(pub u64);

/// An image's id, scoped to its producer.
pub type ImageId = u32;

/// How much a producer is trusted, which decides its budget share and whether the
/// SSH rule applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Unsealed output. `ssh_session` is true on an SSH session, and also when the
    /// terminal cannot tell (it then treats the layer as one).
    Unsigned {
        /// Whether the layer is, or may be, an SSH session.
        ssh_session: bool,
    },
    /// A sealed layer: the whole budget, and no SSH rule.
    Sealed,
}

/// The numbers the spec fixes, as one value so a test can use smaller ones.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Decoded images per session, in bytes.
    pub session_bytes: u64,
    /// The share of the budget an unsigned producer gets, in percent.
    pub unsigned_percent: u64,
    /// Images one producer can have waiting for a decode.
    pub queued_per_producer: usize,
    /// Decodes in flight for one producer.
    pub in_flight_per_producer: usize,
    /// Decodes in flight across the terminal.
    pub in_flight_total: usize,
    /// GPU textures, in bytes.
    pub texture_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            session_bytes: 256 * 1024 * 1024,
            unsigned_percent: 25,
            queued_per_producer: 8,
            in_flight_per_producer: 1,
            in_flight_total: 2,
            texture_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Why the alt text is shown instead of the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltReason {
    /// The setting is off, or no gate record matches this build and platform.
    Locked,
    /// An unsigned image on an SSH session.
    SshSession,
    /// The declared size is zero, over a side or over the pixel limit.
    InvalidHint,
    /// The producer already has the most images waiting.
    QueueFull,
    /// The reservation does not fit the budget; the producer is now degraded.
    OverBudget,
    /// The producer is on alt text for the rest of the command.
    Degraded,
    /// The id is already in use by this producer.
    DuplicateId,
    /// The decoder declined (not a crash).
    Rejected(Status),
    /// The input was refused before a worker started (format mismatch, too large).
    Refused,
    /// The decoder crashed; the producer is now degraded.
    DecoderCrash(Crash),
    /// The decoded image does not fit the texture budget.
    TextureBudget,
    /// The sink could not take the upload.
    UploadFailed,
}

/// A texture the sink holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u64);

/// A sink could not take an upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadFailed;

/// Where decoded pixels go (the renderer, or a headless store).
pub trait TextureSink {
    /// Takes `rgba` (`width * height * 4` bytes, premultiplied, top-down).
    ///
    /// # Errors
    /// [`UploadFailed`] if the pixels cannot be taken.
    fn upload(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureId, UploadFailed>;
    /// Frees a texture.
    fn release(&mut self, texture: TextureId);
}

/// Runs a decode. The real one starts a worker process.
pub trait Decode: Send + Sync + 'static {
    /// Decodes `bytes` of `format` with the declared size `hint`.
    fn decode(&self, format: Format, hint: (u32, u32), bytes: &[u8]) -> Outcome;
}

impl Decode for Runner {
    fn decode(&self, format: Format, hint: (u32, u32), bytes: &[u8]) -> Outcome {
        Runner::decode(self, format, hint, bytes)
    }
}

/// What the build knows about whether images may be shown.
#[derive(Clone, Debug)]
pub struct Gate {
    /// The local `images` setting.
    pub setting_on: bool,
    /// This platform.
    pub platform: String,
    /// This build's two identities.
    pub identities: Identities,
    /// The gate results the build was shipped with.
    pub records: Vec<GateRecord>,
}

impl Gate {
    fn open(&self) -> bool {
        images_enabled(
            self.setting_on,
            &self.platform,
            &self.identities,
            &self.records,
        )
    }
}

/// What happened to a submitted image.
#[derive(Debug, PartialEq, Eq)]
pub enum Submit {
    /// Accepted: it is waiting for or in a decode, and [`ImageManager::poll`]
    /// reports the result.
    Queued,
    /// Not decoded; show the alt text.
    AltText(AltReason),
}

/// A finished image, from [`ImageManager::poll`].
#[derive(Debug, PartialEq, Eq)]
pub struct Finished {
    /// Who sent it.
    pub producer: Producer,
    /// Its id.
    pub id: ImageId,
    /// The result.
    pub result: Result<Ready, AltReason>,
}

/// A decoded, uploaded image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ready {
    /// The texture.
    pub texture: TextureId,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

struct Job {
    producer: Producer,
    id: ImageId,
    format: Format,
    hint: (u32, u32),
    bytes: Vec<u8>,
    reserved: u64,
}

struct Done {
    producer: Producer,
    id: ImageId,
    outcome: Outcome,
}

#[derive(Default)]
struct ProducerState {
    trust: Option<Trust>,
    reserved: u64,
    waiting: usize,
    in_flight: usize,
    degraded: bool,
    ids: HashSet<ImageId>,
}

/// See the module documentation.
pub struct ImageManager<T: TextureSink> {
    limits: Limits,
    gate: Gate,
    decoder: Arc<dyn Decode>,
    sink: T,
    producers: HashMap<Producer, ProducerState>,
    queue: VecDeque<Job>,
    ready: HashMap<(Producer, ImageId), (TextureId, u64)>,
    in_flight_total: usize,
    textures_bytes: u64,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    jobs: HashMap<(Producer, ImageId), Job>,
}

impl<T: TextureSink> ImageManager<T> {
    /// A manager with the given limits, gate, decoder and sink.
    pub fn new(limits: Limits, gate: Gate, decoder: impl Decode, sink: T) -> Self {
        let (tx, rx) = channel();
        Self {
            limits,
            gate,
            decoder: Arc::new(decoder),
            sink,
            producers: HashMap::new(),
            queue: VecDeque::new(),
            ready: HashMap::new(),
            in_flight_total: 0,
            textures_bytes: 0,
            tx,
            rx,
            jobs: HashMap::new(),
        }
    }

    /// Replaces the gate (a setting changed).
    pub fn set_gate(&mut self, gate: Gate) {
        self.gate = gate;
    }

    /// The sink, for the renderer or a test to look at.
    pub fn sink(&self) -> &T {
        &self.sink
    }

    /// Declares a producer and how far it is trusted. Unknown producers are
    /// treated as unsigned and on an SSH session, the fail-closed choice.
    pub fn register(&mut self, producer: Producer, trust: Trust) {
        self.producers.entry(producer).or_default().trust = Some(trust);
    }

    /// Bytes currently reserved or held across the session.
    #[must_use]
    pub fn session_bytes(&self) -> u64 {
        self.producers.values().map(|p| p.reserved).sum()
    }

    /// A new command starts for `producer`: it is no longer on alt text for the
    /// rest of the previous one. Images already shown stay.
    pub fn end_command(&mut self, producer: Producer) {
        if let Some(p) = self.producers.get_mut(&producer) {
            p.degraded = false;
        }
    }

    fn share(&self, trust: Trust) -> u64 {
        match trust {
            Trust::Sealed => self.limits.session_bytes,
            Trust::Unsigned { .. } => {
                self.limits.session_bytes / 100 * self.limits.unsigned_percent
            }
        }
    }

    /// Submits an image whose bytes have arrived. Nothing is decoded or reserved
    /// unless every rule allows it.
    pub fn submit(
        &mut self,
        producer: Producer,
        id: ImageId,
        format: Format,
        hint: (u32, u32),
        bytes: Vec<u8>,
    ) -> Submit {
        let trust = self
            .producers
            .get(&producer)
            .and_then(|p| p.trust)
            .unwrap_or(Trust::Unsigned { ssh_session: true });
        if !self.gate.open() {
            return Submit::AltText(AltReason::Locked);
        }
        if matches!(trust, Trust::Unsigned { ssh_session: true }) {
            return Submit::AltText(AltReason::SshSession);
        }
        let share = self.share(trust);
        let session_used = self.session_bytes();
        let (session_limit, queued_limit) =
            (self.limits.session_bytes, self.limits.queued_per_producer);
        let state = self.producers.entry(producer).or_default();
        if state.degraded {
            return Submit::AltText(AltReason::Degraded);
        }
        if state.ids.contains(&id) {
            return Submit::AltText(AltReason::DuplicateId);
        }
        let Some(reserve) = limits::rgba_bytes(u64::from(hint.0), u64::from(hint.1)) else {
            return Submit::AltText(AltReason::InvalidHint);
        };
        if state.waiting >= queued_limit {
            return Submit::AltText(AltReason::QueueFull);
        }
        let producer_total = state.reserved.checked_add(reserve);
        let session_total = session_used.checked_add(reserve);
        let fits = producer_total.is_some_and(|t| t <= share)
            && session_total.is_some_and(|t| t <= session_limit);
        if !fits {
            // Over budget: this producer is on plain text for the rest of the command.
            state.degraded = true;
            return Submit::AltText(AltReason::OverBudget);
        }
        state.reserved += reserve;
        state.waiting += 1;
        state.ids.insert(id);
        self.queue.push_back(Job {
            producer,
            id,
            format,
            hint,
            bytes,
            reserved: reserve,
        });
        self.start_ready();
        Submit::Queued
    }

    /// Starts queued decodes while the in-flight limits allow, oldest first.
    fn start_ready(&mut self) {
        let mut i = 0;
        while i < self.queue.len() && self.in_flight_total < self.limits.in_flight_total {
            let producer = self.queue[i].producer;
            let busy = self
                .producers
                .get(&producer)
                .is_some_and(|p| p.in_flight >= self.limits.in_flight_per_producer);
            if busy {
                i += 1;
                continue;
            }
            let Some(job) = self.queue.remove(i) else {
                break;
            };
            if let Some(p) = self.producers.get_mut(&producer) {
                p.waiting -= 1;
                p.in_flight += 1;
            }
            self.in_flight_total += 1;
            let (tx, decoder) = (self.tx.clone(), Arc::clone(&self.decoder));
            let (id, format, hint, bytes) = (job.id, job.format, job.hint, job.bytes.clone());
            self.jobs.insert((producer, id), job);
            std::thread::spawn(move || {
                // A panic in the decoder call is a crash, not a lost image.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decoder.decode(format, hint, &bytes)
                }))
                .unwrap_or(Outcome::Crash(Crash::Exit));
                let _ = tx.send(Done {
                    producer,
                    id,
                    outcome,
                });
            });
        }
    }

    fn release(&mut self, producer: Producer, bytes: u64) {
        if let Some(p) = self.producers.get_mut(&producer) {
            p.reserved = p.reserved.saturating_sub(bytes);
        }
    }

    fn finish(&mut self, done: Done) -> Finished {
        let Done {
            producer,
            id,
            outcome,
        } = done;
        let job = self.jobs.remove(&(producer, id));
        let reserved = job.as_ref().map_or(0, |j| j.reserved);
        self.in_flight_total = self.in_flight_total.saturating_sub(1);
        if let Some(p) = self.producers.get_mut(&producer) {
            p.in_flight = p.in_flight.saturating_sub(1);
        }
        let fail = |this: &mut Self, reason: AltReason| {
            this.release(producer, reserved);
            if let Some(p) = this.producers.get_mut(&producer) {
                p.ids.remove(&id);
            }
            Finished {
                producer,
                id,
                result: Err(reason),
            }
        };
        match outcome {
            Outcome::Refused => fail(self, AltReason::Refused),
            Outcome::Rejected(status) => fail(self, AltReason::Rejected(status)),
            Outcome::Crash(kind) => {
                if let Some(p) = self.producers.get_mut(&producer) {
                    p.degraded = true;
                }
                fail(self, AltReason::DecoderCrash(kind))
            }
            Outcome::Image(img) => {
                let actual = (img.rgba.len() as u64).min(reserved);
                if self.textures_bytes.saturating_add(actual) > self.limits.texture_bytes {
                    // Size-checked before the sink is touched.
                    return fail(self, AltReason::TextureBudget);
                }
                let Ok(texture) = self.sink.upload(img.width, img.height, &img.rgba) else {
                    return fail(self, AltReason::UploadFailed);
                };
                // Keep only what the image really uses of its reservation.
                self.release(producer, reserved - actual);
                self.textures_bytes += actual;
                self.ready.insert((producer, id), (texture, actual));
                Finished {
                    producer,
                    id,
                    result: Ok(Ready {
                        texture,
                        width: img.width,
                        height: img.height,
                    }),
                }
            }
        }
    }

    /// Collects finished decodes, uploads their pixels and starts waiting ones.
    pub fn poll(&mut self) -> Vec<Finished> {
        let mut out = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            out.push(self.finish(done));
        }
        self.start_ready();
        out
    }

    /// Frees an image that was shown: its reservation, its texture and its id.
    pub fn drop_image(&mut self, producer: Producer, id: ImageId) {
        if let Some((texture, bytes)) = self.ready.remove(&(producer, id)) {
            self.sink.release(texture);
            self.textures_bytes = self.textures_bytes.saturating_sub(bytes);
            self.release(producer, bytes);
            if let Some(p) = self.producers.get_mut(&producer) {
                p.ids.remove(&id);
            }
        }
    }
}
