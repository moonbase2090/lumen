//! Random frame sequences through the receiver and the image manager.
//!
//! The decoder fuzz targets never reached this code: it parses nothing, it keeps
//! state. These runs drive it with long seeded sequences of starts, data frames
//! (in order, out of order, empty, too big), ends of commands, drops and polls,
//! from three producers, and after every step check what the rules promise:
//! nothing held or reserved past its limit, and, once everything is drained,
//! nothing left held, reserved or uploaded, and every queued image answered once.
//! The decoder is a stand-in that picks its answer from a byte of the image, so
//! refusals, rejections, crashes and panics all happen.

#![cfg(any(unix, windows))]
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lumen_decoder::frames::{Data, MAX_CHUNK, MAX_HELD_BYTES, MAX_IMAGE_BYTES, Receiver, Start};
use lumen_decoder::images::{
    Decode, Finished, Gate, ImageId, ImageManager, Limits, Producer, Submit, TextureId,
    TextureSink, Trust, UploadFailed,
};
use lumen_decoder::protocol::{Format, Status};
use lumen_decoder::runner::{Crash, Image, Outcome};
use lumen_decoder::unlock::{GateRecord, Identities};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The byte `i` of an image whose answer is picked by `selector`.
fn payload_byte(selector: u8, i: usize) -> u8 {
    match i {
        0..=7 => PNG[i],
        8 => selector,
        _ => (i % 251) as u8,
    }
}

/// Answers by the byte after the signature: images (twice as likely), a refusal,
/// a rejection, a crash and a panic.
struct Picky;
impl Decode for Picky {
    fn decode(&self, _: Format, _: (u32, u32), bytes: &[u8]) -> Outcome {
        match bytes.get(8).copied().unwrap_or(0) % 6 {
            0 | 1 => Outcome::Image(Image {
                width: 1,
                height: 1,
                rgba: vec![9, 9, 9, 255],
            }),
            2 => Outcome::Refused,
            3 => Outcome::Rejected(Status::Corrupt),
            4 => Outcome::Crash(Crash::Exit),
            #[allow(clippy::panic)]
            _ => panic!("the stand-in decoder panics on purpose"),
        }
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<(u64, HashSet<u64>)>>);
impl Sink {
    fn live(&self) -> usize {
        self.0.lock().map_or(usize::MAX, |s| s.1.len())
    }
}
impl TextureSink for Sink {
    fn upload(&mut self, _: u32, _: u32, _: &[u8]) -> Result<TextureId, UploadFailed> {
        let mut s = self.0.lock().map_err(|_| UploadFailed)?;
        s.0 += 1;
        let id = s.0;
        s.1.insert(id);
        Ok(TextureId(id))
    }
    fn release(&mut self, texture: TextureId) {
        if let Ok(mut s) = self.0.lock() {
            assert!(s.1.remove(&texture.0), "a texture was released twice");
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn gate() -> Gate {
    let ids = Identities {
        decoder_build: "d".into(),
        image_path_build: "p".into(),
    };
    Gate {
        setting_on: true,
        platform: "p".into(),
        identities: ids.clone(),
        records: vec![GateRecord {
            platform: "p".into(),
            decoder_build: ids.decoder_build,
            image_path_build: ids.image_path_build,
            decoder_gate_passed: true,
            memory_gate_passed: true,
        }],
    }
}

/// What the producer's current image start said, so data frames can continue it.
#[derive(Clone, Copy)]
struct Sent {
    id: ImageId,
    length: u32,
    selector: u8,
    next: u32,
}

struct World {
    limits: Limits,
    manager: ImageManager<Sink>,
    sink: Sink,
    receiver: Receiver,
    producers: [Producer; 3],
    sent: HashMap<Producer, Sent>,
    /// Queued for a decode and not yet answered.
    pending: HashSet<(Producer, ImageId)>,
    ready: HashSet<(Producer, ImageId)>,
}

impl World {
    fn new() -> Self {
        let limits = Limits {
            session_bytes: 4 * 1024 * 1024,
            // Room for sixteen of the stand-in's 4 byte images, so the budget
            // is reached and its accounting matters.
            texture_bytes: 64,
            ..Limits::default()
        };
        let sink = Sink::default();
        let mut manager = ImageManager::new(limits, gate(), Picky, sink.clone());
        let producers = [Producer(1), Producer(2), Producer(3)];
        manager.register(producers[0], Trust::Sealed);
        manager.register(producers[1], Trust::Unsigned { ssh_session: false });
        manager.register(producers[2], Trust::Unsigned { ssh_session: false });
        Self {
            limits,
            manager,
            sink,
            receiver: Receiver::new(),
            producers,
            sent: HashMap::new(),
            pending: HashSet::new(),
            ready: HashSet::new(),
        }
    }

    fn answer(&mut self, finished: Vec<Finished>) {
        for f in finished {
            let key = (f.producer, f.id);
            assert!(
                self.pending.remove(&key),
                "an answer for an image that was not waiting for one: {key:?}"
            );
            if f.result.is_ok() {
                self.ready.insert(key);
            }
        }
    }

    fn start(&mut self, rng: &mut Rng, p: Producer) {
        let id = rng.below(5) as u32;
        let length = match rng.below(10) {
            0 => 0,
            1 => MAX_IMAGE_BYTES + 1 + rng.below(1000) as u32,
            2 => u32::MAX,
            3 => MAX_IMAGE_BYTES,
            _ => 9 + rng.below(40) as u32,
        };
        let side = |rng: &mut Rng| match rng.below(12) {
            0 => 0,
            1 => 70_000,
            2 => 4_000,
            _ => 1 + rng.below(64) as u32,
        };
        let (width, height) = (side(rng), side(rng));
        let selector = rng.below(256) as u8;
        let started = self.receiver.start(
            &mut self.manager,
            p,
            Start {
                id,
                width,
                height,
                length,
            },
        );
        if matches!(
            started.placement,
            lumen_decoder::frames::Placement::Receiving
        ) {
            self.sent.insert(
                p,
                Sent {
                    id,
                    length,
                    selector,
                    next: 0,
                },
            );
        } else {
            self.sent.remove(&p);
        }
    }

    fn data(&mut self, rng: &mut Rng, p: Producer) {
        let cur = self.sent.get(&p).copied();
        let mostly_right = rng.below(10) < 8;
        let (id, offset, len) = match cur {
            Some(s) if mostly_right => (
                s.id,
                s.next,
                (1 + rng.below(30) as u32).min(s.length - s.next),
            ),
            Some(s) => (
                s.id.wrapping_add(rng.below(2) as u32),
                s.next.wrapping_add(rng.below(3) as u32).wrapping_sub(1),
                rng.below(MAX_CHUNK as u64 + 3) as u32,
            ),
            None => (
                rng.below(5) as u32,
                rng.below(20) as u32,
                rng.below(40) as u32,
            ),
        };
        let selector = cur.map_or(0, |s| s.selector);
        let bytes: Vec<u8> = (0..len as usize)
            .map(|i| payload_byte(selector, offset as usize + i))
            .collect();
        let got = self.receiver.data(&mut self.manager, p, id, offset, &bytes);
        match got {
            Data::Buffered => {
                if let Some(s) = self.sent.get_mut(&p) {
                    s.next += len;
                }
            }
            Data::Submitted(done, submit) => {
                self.sent.remove(&p);
                if submit == Submit::Queued {
                    let key = (p, done);
                    assert!(
                        !self.pending.contains(&key) && !self.ready.contains(&key),
                        "an id queued while it was still in use"
                    );
                    self.pending.insert(key);
                }
            }
            Data::Dropped => {}
            Data::Rejected { .. } => {
                self.sent.remove(&p);
            }
        }
    }

    fn step(&mut self, rng: &mut Rng) {
        let p = self.producers[rng.below(3) as usize];
        match rng.below(20) {
            0..=3 => self.start(rng, p),
            4..=13 => self.data(rng, p),
            14 => {
                self.receiver.end(&mut self.manager, p);
                self.sent.remove(&p);
            }
            15 | 16 => {
                let f = self.manager.poll();
                self.answer(f);
            }
            17 | 18 => {
                let shown: Vec<_> = self.ready.iter().copied().collect();
                if let Some(&(owner, id)) = shown.get(rng.below(shown.len().max(1) as u64) as usize)
                {
                    self.manager.drop_image(owner, id);
                    self.ready.remove(&(owner, id));
                }
            }
            _ => self.manager.end_command(p),
        }
        self.check();
    }

    fn check(&self) {
        assert!(
            self.manager.session_bytes() <= self.limits.session_bytes,
            "the session reserved {} bytes, over its {}",
            self.manager.session_bytes(),
            self.limits.session_bytes
        );
        for &p in &self.producers {
            assert!(
                self.manager.held_bytes(p) <= MAX_HELD_BYTES,
                "a producer holds {} encoded bytes",
                self.manager.held_bytes(p)
            );
            assert!(self.receiver.receiving_bytes(p) <= u64::from(MAX_IMAGE_BYTES));
        }
    }

    /// Collects answers until every decode the manager started has been
    /// answered. The manager has no blocking wait (adding one changes the image
    /// path's build identity, and with it the gate that was run for it), so this
    /// asks again, yielding in between, with no timer; it ends when the decoders
    /// have all returned, which they do, the stand-in being immediate.
    fn settle(&mut self) -> Vec<Finished> {
        let mut all = Vec::new();
        loop {
            all.extend(self.manager.poll());
            let answered = all
                .iter()
                .filter(|f| self.pending.contains(&(f.producer, f.id)))
                .count();
            if answered == self.pending.len() {
                return all;
            }
            std::thread::yield_now();
        }
    }

    /// With everything freed, a fresh image must be accepted and shown: nothing
    /// is left counted against the texture budget or the producer.
    fn probe(&mut self) {
        let p = self.producers[0];
        let bytes: Vec<u8> = (0..9).map(|i| payload_byte(0, i)).collect();
        let started = self.receiver.start(
            &mut self.manager,
            p,
            Start {
                id: 0,
                width: 1,
                height: 1,
                length: 9,
            },
        );
        assert!(matches!(
            started.placement,
            lumen_decoder::frames::Placement::Receiving
        ));
        let got = self.receiver.data(&mut self.manager, p, 0, 0, &bytes);
        assert!(matches!(got, Data::Submitted(0, Submit::Queued)), "{got:?}");
        self.pending.insert((p, 0));
        let finished = self.settle();
        assert!(
            matches!(finished.as_slice(), [f] if f.result.is_ok()),
            "the probe image was not shown: {finished:?}"
        );
        self.manager.drop_image(p, 0);
    }

    fn drain(&mut self) {
        let f = self.settle();
        self.answer(f);
        for &p in &self.producers.clone() {
            self.receiver.end(&mut self.manager, p);
        }
        let f = self.manager.poll();
        self.answer(f);
        assert!(
            self.pending.is_empty(),
            "images queued and never answered: {:?}",
            self.pending
        );
        for (p, id) in self.ready.drain().collect::<Vec<_>>() {
            self.manager.drop_image(p, id);
        }
        assert_eq!(self.manager.session_bytes(), 0, "bytes still reserved");
        self.probe();
        assert_eq!(self.sink.live(), 0, "textures still uploaded");
        for &p in &self.producers {
            assert_eq!(self.manager.held_bytes(p), 0, "encoded bytes still held");
            for id in 0..5 {
                assert!(
                    !self.manager.has_id(p, id),
                    "id {id} still in use after a full drain"
                );
            }
            assert_eq!(self.receiver.receiving_bytes(p), 0);
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let mut w = World::new();
    for _ in 0..steps {
        w.step(&mut rng);
    }
    w.drain();
}

#[test]
fn random_frame_sequences_keep_every_limit_and_free_everything() {
    for seed in 1..=12u64 {
        run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 4000);
    }
}
