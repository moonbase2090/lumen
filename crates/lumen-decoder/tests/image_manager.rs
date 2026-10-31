//! The image manager: unlock, the SSH rule, reservation before decode, queue and
//! concurrency limits, crash and over-budget degradation, and size-checked uploads.
//!
//! Scheduling and budget tests use a decoder that blocks until the test lets it go
//! (it stands in for the process the manager starts, not for the manager). The
//! end-to-end tests use the real worker.

#![cfg(unix)]
#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use lumen_decoder::images::{
    AltReason, Decode, Finished, Gate, ImageManager, Limits, Producer, Submit, TextureId,
    TextureSink, Trust, UploadFailed,
};
use lumen_decoder::protocol::{Format, Status};
use lumen_decoder::runner::{Crash, Image, Outcome, Runner};
use lumen_decoder::unlock::{GateRecord, Identities};

const WORKER: &str = env!("CARGO_BIN_EXE_lumen-decoder-worker");
const HOSTILE: &str = env!("CARGO_BIN_EXE_lumen-decoder-hostile");

// ---- test helpers -------------------------------------------------------

#[derive(Default)]
struct SinkLog {
    uploads: Vec<(u32, u32, Vec<u8>)>,
    released: Vec<TextureId>,
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<SinkLog>>);

impl TextureSink for Sink {
    fn upload(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureId, UploadFailed> {
        let mut log = self.0.lock().map_err(|_| UploadFailed)?;
        log.uploads.push((width, height, rgba.to_vec()));
        Ok(TextureId(log.uploads.len() as u64))
    }
    fn release(&mut self, texture: TextureId) {
        if let Ok(mut log) = self.0.lock() {
            log.released.push(texture);
        }
    }
}

impl Sink {
    fn uploads(&self) -> usize {
        self.0.lock().map_or(0, |l| l.uploads.len())
    }
    fn first_pixel(&self) -> [u8; 4] {
        self.0
            .lock()
            .ok()
            .and_then(|l| {
                l.uploads
                    .first()
                    .and_then(|u| u.2.get(..4).map(|b| [b[0], b[1], b[2], b[3]]))
            })
            .unwrap_or([0; 4])
    }
    fn released(&self) -> usize {
        self.0.lock().map_or(0, |l| l.released.len())
    }
}

fn gate(open: bool) -> Gate {
    let ids = Identities {
        decoder_build: "d1".into(),
        image_path_build: "p1".into(),
    };
    let records = if open {
        vec![GateRecord {
            platform: "linux".into(),
            decoder_build: "d1".into(),
            image_path_build: "p1".into(),
            decoder_gate_passed: true,
            memory_gate_passed: true,
        }]
    } else {
        Vec::new()
    };
    Gate {
        setting_on: true,
        platform: "linux".into(),
        identities: ids,
        records,
    }
}

/// Blocks each decode until released, and records how many ran at once, per
/// producer (the producer is the first byte of the image) and in total.
#[derive(Default)]
struct Held {
    open: Mutex<bool>,
    cv: Condvar,
    running: Mutex<HashMap<u8, usize>>,
    total: AtomicUsize,
    max_total: AtomicUsize,
    max_per_producer: AtomicUsize,
    started: AtomicUsize,
    block: AtomicBool,
}

#[derive(Clone)]
struct HeldDecoder(Arc<Held>);

impl HeldDecoder {
    fn new() -> Self {
        let h = Held::default();
        h.block.store(true, Ordering::SeqCst);
        Self(Arc::new(h))
    }
    fn release(&self) {
        self.0.block.store(false, Ordering::SeqCst);
        if let Ok(mut open) = self.0.open.lock() {
            *open = true;
        }
        self.0.cv.notify_all();
    }
}

impl Decode for HeldDecoder {
    fn decode(&self, _format: Format, hint: (u32, u32), bytes: &[u8]) -> Outcome {
        let who = bytes.first().copied().unwrap_or(0);
        let h = &self.0;
        h.started.fetch_add(1, Ordering::SeqCst);
        let now = h.total.fetch_add(1, Ordering::SeqCst) + 1;
        h.max_total.fetch_max(now, Ordering::SeqCst);
        if let Ok(mut r) = h.running.lock() {
            let n = r.entry(who).or_insert(0);
            *n += 1;
            h.max_per_producer.fetch_max(*n, Ordering::SeqCst);
        }
        if let Ok(mut open) = h.open.lock() {
            while !*open {
                open =
                    h.cv.wait(open)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
        if let Ok(mut r) = h.running.lock()
            && let Some(n) = r.get_mut(&who)
        {
            *n -= 1;
        }
        h.total.fetch_sub(1, Ordering::SeqCst);
        // A small real image whatever the hint said: the reservation then shrinks.
        let (w, h2) = (hint.0.min(2), hint.1.min(2));
        Outcome::Image(Image {
            width: w,
            height: h2,
            rgba: vec![7; (w * h2 * 4) as usize],
        })
    }
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

fn collect<T: TextureSink>(m: &mut ImageManager<T>, want: usize) -> Vec<Finished> {
    let mut all = Vec::new();
    wait_until(|| {
        all.extend(m.poll());
        all.len() >= want
    });
    all
}

fn png(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = enc.write_header() {
            let data: Vec<u8> = (0..w * h).flat_map(|_| px).collect();
            let _ = wr.write_image_data(&data);
        }
    }
    out
}

/// The first byte stands for the producer in `HeldDecoder` tests.
fn bytes_for(who: u8) -> Vec<u8> {
    vec![who, 0, 0]
}

const SEALED: Trust = Trust::Sealed;
const UNSIGNED: Trust = Trust::Unsigned { ssh_session: false };

fn held(limits: Limits) -> (ImageManager<Sink>, HeldDecoder, Sink) {
    let d = HeldDecoder::new();
    let s = Sink::default();
    (
        ImageManager::new(limits, gate(true), d.clone(), s.clone()),
        d,
        s,
    )
}

// ---- end to end with the real worker -----------------------------------

#[test]
fn a_png_goes_through_the_real_worker_to_a_texture_with_the_exact_pixels() {
    let sink = Sink::default();
    let runner = Runner::new(WORKER).with_wall(Duration::from_secs(60));
    let mut m = ImageManager::new(Limits::default(), gate(true), runner, sink.clone());
    m.register(Producer(1), SEALED);
    assert_eq!(
        m.submit(
            Producer(1),
            7,
            Format::Png,
            (3, 2),
            png(3, 2, [200, 100, 50, 128])
        ),
        Submit::Queued
    );
    let done = collect(&mut m, 1);
    assert_eq!(done.len(), 1);
    let none = lumen_decoder::images::Ready {
        texture: TextureId(0),
        width: 0,
        height: 0,
    };
    let ready = done[0].result.unwrap_or(none);
    assert_eq!((ready.width, ready.height), (3, 2));
    assert_eq!(sink.uploads(), 1);
    // Premultiplied: 200 * 128 / 255 = 100, 100 * 128 / 255 = 50, 50 * 128 / 255 = 25.
    assert_eq!(sink.first_pixel(), [100, 50, 25, 128]);
}

#[test]
fn a_decoder_crash_puts_the_producer_on_alt_text_for_the_rest_of_the_command() {
    let sink = Sink::default();
    let runner = Runner::new(HOSTILE)
        .with_args(&["raw-exit-nonzero"])
        .with_wall(Duration::from_secs(60));
    let mut m = ImageManager::new(Limits::default(), gate(true), runner, sink.clone());
    m.register(Producer(1), SEALED);
    assert_eq!(
        m.submit(
            Producer(1),
            1,
            Format::Png,
            (1, 1),
            png(1, 1, [0, 0, 0, 255])
        ),
        Submit::Queued
    );
    let done = collect(&mut m, 1);
    assert_eq!(done[0].result, Err(AltReason::DecoderCrash(Crash::Exit)));
    // Later images of the same command are not even tried.
    assert_eq!(
        m.submit(
            Producer(1),
            2,
            Format::Png,
            (1, 1),
            png(1, 1, [0, 0, 0, 255])
        ),
        Submit::AltText(AltReason::Degraded)
    );
    // Another producer is unaffected, and a new command clears it.
    m.register(Producer(2), SEALED);
    assert_eq!(
        m.submit(
            Producer(2),
            1,
            Format::Png,
            (1, 1),
            png(1, 1, [0, 0, 0, 255])
        ),
        Submit::Queued
    );
    m.end_command(Producer(1));
    assert_eq!(
        m.submit(
            Producer(1),
            3,
            Format::Png,
            (1, 1),
            png(1, 1, [0, 0, 0, 255])
        ),
        Submit::Queued
    );
    assert_eq!(sink.uploads(), 0);
}

#[test]
fn a_decoder_that_declines_is_alt_text_but_not_a_crash() {
    let runner = Runner::new(WORKER).with_wall(Duration::from_secs(60));
    let mut m = ImageManager::new(Limits::default(), gate(true), runner, Sink::default());
    m.register(Producer(1), SEALED);
    let mut bad = png(8, 8, [1, 2, 3, 255]);
    bad.truncate(bad.len() / 2);
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (8, 8), bad),
        Submit::Queued
    );
    let done = collect(&mut m, 1);
    assert_eq!(done[0].result, Err(AltReason::Rejected(Status::Corrupt)));
    // Not degraded: the next image is accepted.
    assert_eq!(
        m.submit(
            Producer(1),
            2,
            Format::Png,
            (1, 1),
            png(1, 1, [0, 0, 0, 255])
        ),
        Submit::Queued
    );
}

// ---- unlock and the SSH rule -------------------------------------------

#[test]
fn nothing_is_decoded_while_the_gate_is_closed() {
    for (setting, open) in [(true, false), (false, true)] {
        let d = HeldDecoder::new();
        let mut g = gate(open);
        g.setting_on = setting;
        let mut m = ImageManager::new(Limits::default(), g, d.clone(), Sink::default());
        m.register(Producer(1), SEALED);
        assert_eq!(
            m.submit(Producer(1), 1, Format::Png, (4, 4), bytes_for(1)),
            Submit::AltText(AltReason::Locked)
        );
        assert_eq!(m.session_bytes(), 0, "no reservation either");
        assert_eq!(d.0.started.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn an_unsigned_image_on_an_ssh_session_is_never_decoded() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), Trust::Unsigned { ssh_session: true });
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (4, 4), bytes_for(1)),
        Submit::AltText(AltReason::SshSession)
    );
    // A producer nobody registered is treated the same way: fail closed.
    assert_eq!(
        m.submit(Producer(9), 1, Format::Png, (4, 4), bytes_for(9)),
        Submit::AltText(AltReason::SshSession)
    );
    // A sealed layer is not subject to it.
    m.register(Producer(2), SEALED);
    assert_eq!(
        m.submit(Producer(2), 1, Format::Png, (4, 4), bytes_for(2)),
        Submit::Queued
    );
    // Only the sealed one is ever started; the SSH ones never reach a decoder.
    assert!(wait_until(|| d.0.started.load(Ordering::SeqCst) == 1));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(d.0.started.load(Ordering::SeqCst), 1);
    d.release();
}

// ---- the declared size is only a hint ----------------------------------

#[test]
fn a_declared_size_over_the_limits_is_refused_before_anything_is_reserved() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    for hint in [
        (0, 5),
        (5, 0),
        (65536, 1),
        (1, 65536),
        (4001, 4000),
        (641, 24961),
        (u32::MAX, u32::MAX),
    ] {
        assert_eq!(
            m.submit(Producer(1), 1, Format::Png, hint, bytes_for(1)),
            Submit::AltText(AltReason::InvalidHint),
            "{hint:?}"
        );
    }
    assert_eq!(m.session_bytes(), 0);
    assert_eq!(d.0.started.load(Ordering::SeqCst), 0);
}

// ---- reservation and budgets -------------------------------------------

#[test]
fn the_reservation_is_taken_at_submit_and_shrinks_to_the_decoded_size() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (1000, 1000), bytes_for(1)),
        Submit::Queued
    );
    assert_eq!(
        m.session_bytes(),
        4_000_000,
        "w * h * 4 before any decode finishes"
    );
    d.release();
    let done = collect(&mut m, 1);
    assert!(done[0].result.is_ok());
    assert_eq!(m.session_bytes(), 16, "the 2x2 image it really was");
}

#[test]
fn queued_images_count_against_the_budget() {
    let (mut m, _d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    // The decoder is blocked, so everything stays queued or in flight.
    for id in 0..2 {
        assert_eq!(
            m.submit(Producer(1), id, Format::Png, (4000, 4000), bytes_for(1)),
            Submit::Queued
        );
    }
    assert_eq!(m.session_bytes(), 128_000_000);
}

#[test]
fn an_unsigned_producer_gets_a_quarter_of_the_budget_and_degrades_past_it() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), UNSIGNED);
    // 25% of 256 MiB is 67,108,864 bytes. 4000 x 4000 x 4 = 64,000,000 fits once.
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (4000, 4000), bytes_for(1)),
        Submit::Queued
    );
    assert_eq!(
        m.submit(Producer(1), 2, Format::Png, (1000, 1000), bytes_for(1)),
        Submit::AltText(AltReason::OverBudget)
    );
    // Degraded for the rest of the command, even for a tiny image.
    assert_eq!(
        m.submit(Producer(1), 3, Format::Png, (1, 1), bytes_for(1)),
        Submit::AltText(AltReason::Degraded)
    );
    m.end_command(Producer(1));
    assert_eq!(
        m.submit(Producer(1), 4, Format::Png, (1, 1), bytes_for(1)),
        Submit::Queued
    );
    d.release();
}

#[test]
fn a_sealed_producer_may_use_the_whole_session_budget_but_not_more() {
    let (mut m, _d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    m.register(Producer(2), SEALED);
    // 4000 x 4000 x 4 = 64,000,000: four fit in 268,435,456, a fifth does not.
    for (producer, id) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
        assert_eq!(
            m.submit(
                Producer(producer),
                id,
                Format::Png,
                (4000, 4000),
                bytes_for(1)
            ),
            Submit::Queued,
            "producer {producer}, image {id}"
        );
    }
    assert_eq!(
        m.submit(Producer(1), 3, Format::Png, (4000, 4000), bytes_for(1)),
        Submit::AltText(AltReason::OverBudget)
    );
}

// ---- the largest legal image ---------------------------------------------

/// The pixel counts that are the largest legal image, in every shape the side
/// limit allows that the spec's numbers make easy to write down.
const LARGEST: [(u32, u32); 4] = [(4000, 4000), (3200, 5000), (250, 64000), (64000, 250)];

#[test]
fn the_largest_legal_image_fits_one_producers_whole_budget_whatever_its_shape() {
    use lumen_decoder::limits;
    // The number the spec fixes, and the share of one unsigned producer it must fit.
    let limits_default = Limits::default();
    let unsigned_share = limits_default.session_bytes / 100 * limits_default.unsigned_percent;
    assert_eq!(limits::MAX_PIXELS, 16_000_000);
    assert_eq!(limits::MAX_PIXELS * 4, 64_000_000);
    assert!(limits::MAX_PIXELS * 4 <= unsigned_share, "{unsigned_share}");
    for (trust, name) in [(UNSIGNED, "unsigned"), (SEALED, "sealed")] {
        for hint in LARGEST {
            assert_eq!(u64::from(hint.0) * u64::from(hint.1), limits::MAX_PIXELS);
            let (mut m, d, _s) = held(Limits::default());
            m.register(Producer(1), trust);
            assert_eq!(
                m.submit(Producer(1), 1, Format::Png, hint, bytes_for(1)),
                Submit::Queued,
                "{name} {hint:?}: the largest legal image was not accepted"
            );
            assert_eq!(m.session_bytes(), 64_000_000, "{name} {hint:?}");
            drop(d);
        }
    }
}

#[test]
fn an_image_one_pixel_over_the_limit_is_refused_before_anything_is_reserved_or_decoded() {
    // 641 x 24961 = 16,000,001: one pixel over, and inside the side limit.
    assert_eq!(641_u64 * 24_961, 16_000_001);
    for (trust, name) in [(UNSIGNED, "unsigned"), (SEALED, "sealed")] {
        let (mut m, d, _s) = held(Limits::default());
        m.register(Producer(1), trust);
        assert_eq!(
            m.submit(Producer(1), 1, Format::Png, (641, 24961), bytes_for(1)),
            Submit::AltText(AltReason::InvalidHint),
            "{name}"
        );
        assert_eq!(m.session_bytes(), 0, "{name}: nothing was reserved");
        assert_eq!(
            d.0.started.load(Ordering::SeqCst),
            0,
            "{name}: a decoder was started"
        );
        // And the refusal does not degrade the producer: a legal image is accepted next.
        assert_eq!(
            m.submit(Producer(1), 2, Format::Png, (4000, 4000), bytes_for(1)),
            Submit::Queued,
            "{name}"
        );
    }
}

// ---- queue and concurrency ---------------------------------------------

#[test]
fn a_producer_has_one_decode_in_flight_and_eight_waiting_and_the_tenth_is_alt_text() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    assert_eq!(
        m.submit(Producer(1), 0, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    assert!(
        wait_until(|| d.0.started.load(Ordering::SeqCst) == 1),
        "the first one starts"
    );
    for id in 1..=8 {
        assert_eq!(
            m.submit(Producer(1), id, Format::Png, (10, 10), bytes_for(1)),
            Submit::Queued,
            "waiting #{id}"
        );
    }
    assert_eq!(
        m.submit(Producer(1), 9, Format::Png, (10, 10), bytes_for(1)),
        Submit::AltText(AltReason::QueueFull)
    );
    d.release();
    let done = collect(&mut m, 9);
    assert_eq!(done.len(), 9);
    assert!(done.iter().all(|f| f.result.is_ok()));
    assert_eq!(
        d.0.max_per_producer.load(Ordering::SeqCst),
        1,
        "never two for one producer"
    );
}

#[test]
fn at_most_two_decodes_run_across_the_terminal() {
    let (mut m, d, _s) = held(Limits::default());
    for who in 1..=4u8 {
        m.register(Producer(u64::from(who)), SEALED);
        assert_eq!(
            m.submit(
                Producer(u64::from(who)),
                0,
                Format::Png,
                (10, 10),
                bytes_for(who)
            ),
            Submit::Queued
        );
    }
    assert!(
        wait_until(|| d.0.started.load(Ordering::SeqCst) == 2),
        "two start"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(d.0.started.load(Ordering::SeqCst), 2, "the other two wait");
    d.release();
    let done = collect(&mut m, 4);
    assert_eq!(done.len(), 4);
    assert_eq!(d.0.max_total.load(Ordering::SeqCst), 2);
}

#[test]
fn a_duplicate_id_from_one_producer_is_refused_and_the_same_id_from_another_is_not() {
    let (mut m, d, _s) = held(Limits::default());
    m.register(Producer(1), SEALED);
    m.register(Producer(2), SEALED);
    assert_eq!(
        m.submit(Producer(1), 5, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    assert_eq!(
        m.submit(Producer(1), 5, Format::Png, (10, 10), bytes_for(1)),
        Submit::AltText(AltReason::DuplicateId)
    );
    assert_eq!(
        m.submit(Producer(2), 5, Format::Png, (10, 10), bytes_for(2)),
        Submit::Queued,
        "ids are scoped per producer"
    );
    d.release();
}

// ---- upload and the texture budget -------------------------------------

#[test]
fn an_upload_that_would_pass_the_texture_budget_is_never_made() {
    let limits = Limits {
        texture_bytes: 20,
        ..Limits::default()
    };
    let (mut m, d, sink) = held(limits);
    m.register(Producer(1), SEALED);
    // Each decoded image is 2 x 2 x 4 = 16 bytes: one fits in 20, the second does not.
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    assert_eq!(
        m.submit(Producer(1), 2, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    d.release();
    let done = collect(&mut m, 2);
    let ok = done.iter().filter(|f| f.result.is_ok()).count();
    assert_eq!(ok, 1);
    assert!(
        done.iter()
            .any(|f| f.result == Err(AltReason::TextureBudget))
    );
    assert_eq!(
        sink.uploads(),
        1,
        "the sink was not called for the one that did not fit"
    );
}

#[test]
fn dropping_an_image_frees_its_texture_budget_and_its_id() {
    let limits = Limits {
        texture_bytes: 20,
        ..Limits::default()
    };
    let (mut m, d, sink) = held(limits);
    m.register(Producer(1), SEALED);
    d.release();
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    assert_eq!(collect(&mut m, 1).len(), 1);
    assert_eq!(m.session_bytes(), 16);
    m.drop_image(Producer(1), 1);
    assert_eq!(sink.released(), 1);
    assert_eq!(m.session_bytes(), 0);
    // The same id and the texture budget are free again.
    assert_eq!(
        m.submit(Producer(1), 1, Format::Png, (10, 10), bytes_for(1)),
        Submit::Queued
    );
    let done = collect(&mut m, 1);
    assert!(done[0].result.is_ok());
}
