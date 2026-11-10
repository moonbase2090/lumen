//! Receiving an image from its frames: order, caps, nothing buffered while
//! locked, the format from the bytes, and the end of a command.

#![cfg(any(unix, windows))]
#![allow(missing_docs)]
// The sizes in these tests are small constants.
#![allow(clippy::cast_possible_truncation)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_decoder::frames::{Data, MAX_HELD_BYTES, MAX_IMAGE_BYTES, Placement, Receiver, Start};
use lumen_decoder::images::{
    AltReason, Decode, Gate, ImageManager, Limits, Producer, Submit, TextureId, TextureSink, Trust,
    UploadFailed,
};
use lumen_decoder::protocol::{Format, Status};
use lumen_decoder::runner::{Image, Outcome};
use lumen_decoder::unlock::{GateRecord, Identities};

const P: Producer = Producer(1);
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

type Call = (Format, (u32, u32), usize);

#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<Call>>>);

impl Calls {
    fn all(&self) -> Vec<Call> {
        self.0.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Decode for Calls {
    fn decode(&self, format: Format, hint: (u32, u32), bytes: &[u8]) -> Outcome {
        if let Ok(mut c) = self.0.lock() {
            c.push((format, hint, bytes.len()));
        }
        Outcome::Image(Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        })
    }
}

struct Sink;
impl TextureSink for Sink {
    fn upload(&mut self, _: u32, _: u32, _: &[u8]) -> Result<TextureId, UploadFailed> {
        Ok(TextureId(1))
    }
    fn release(&mut self, _: TextureId) {}
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

fn manager(open: bool) -> (ImageManager<Sink>, Calls) {
    let calls = Calls::default();
    let mut m = ImageManager::new(Limits::default(), gate(open), calls.clone(), Sink);
    m.register(P, Trust::Unsigned { ssh_session: false });
    (m, calls)
}

fn start(id: u32, length: u32) -> Start {
    Start {
        id,
        width: 4,
        height: 4,
        length,
    }
}

fn png(len: usize) -> Vec<u8> {
    let mut v = PNG.to_vec();
    v.resize(len, 0);
    v
}

fn finished(m: &mut ImageManager<Sink>) -> usize {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let done = m.poll();
        if !done.is_empty() {
            return done.len();
        }
        assert!(Instant::now() < end, "decode did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_image_in_order_is_submitted_with_the_format_of_its_bytes() {
    let (mut m, calls) = manager(true);
    let mut r = Receiver::new();
    let bytes = png(100);
    let s = r.start(&mut m, P, start(7, 100));
    assert_eq!(s.placement, Placement::Receiving);
    assert_eq!(r.data(&mut m, P, 7, 0, &bytes[..60]), Data::Buffered);
    assert_eq!(r.receiving_bytes(P), 60);
    let d = r.data(&mut m, P, 7, 60, &bytes[60..]);
    assert_eq!(d, Data::Submitted(7, Submit::Queued));
    assert_eq!(finished(&mut m), 1);
    assert_eq!(calls.all(), vec![(Format::Png, (4, 4), 100)]);
}

#[test]
fn the_format_comes_from_the_bytes_and_unknown_bytes_are_refused() {
    let (mut m, calls) = manager(true);
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, 20));
    let d = r.data(&mut m, P, 1, 0, &[0u8; 20]);
    assert_eq!(d, Data::Submitted(1, Submit::AltText(AltReason::Refused)));
    assert_eq!(calls.all(), Vec::<Call>::new());
}

#[test]
fn nothing_is_buffered_while_images_are_locked() {
    let (mut m, calls) = manager(false);
    let mut r = Receiver::new();
    let s = r.start(&mut m, P, start(1, 100));
    assert_eq!(s.placement, Placement::Alt(AltReason::Locked));
    for off in [0u32, 50] {
        assert_eq!(r.data(&mut m, P, 1, off, &png(50)), Data::Dropped);
    }
    assert_eq!(r.receiving_bytes(P), 0);
    assert_eq!(calls.all(), Vec::<Call>::new());
}

#[test]
fn an_unknown_id_is_rejected_and_ends_nothing() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    assert_eq!(
        r.data(&mut m, P, 9, 0, &png(10)),
        Data::Rejected { abandoned: None }
    );
}

#[test]
fn a_gap_an_overlap_an_overrun_and_a_wrong_id_end_the_image() {
    for (id, off, len) in [(1u32, 10u32, 10usize), (1, 0, 10), (1, 40, 61), (2, 40, 10)] {
        let (mut m, calls) = manager(true);
        let mut r = Receiver::new();
        r.start(&mut m, P, start(1, 100));
        assert_eq!(r.data(&mut m, P, 1, 0, &png(40)), Data::Buffered);
        let d = r.data(&mut m, P, id, off, &vec![0u8; len]);
        assert_eq!(d, Data::Rejected { abandoned: Some(1) }, "{id} {off} {len}");
        assert_eq!(r.receiving_bytes(P), 0);
        // The ended image cannot be completed afterwards.
        assert_eq!(
            r.data(&mut m, P, 1, 40, &[0u8; 60]),
            Data::Rejected { abandoned: None }
        );
        assert_eq!(calls.all(), Vec::<Call>::new());
    }
}

#[test]
fn an_empty_or_oversize_chunk_is_rejected() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, MAX_IMAGE_BYTES));
    assert_eq!(
        r.data(&mut m, P, 1, 0, &[]),
        Data::Rejected { abandoned: Some(1) }
    );
    r.start(&mut m, P, start(2, MAX_IMAGE_BYTES));
    let big = vec![0u8; lumen_decoder::frames::MAX_CHUNK + 1];
    assert_eq!(
        r.data(&mut m, P, 2, 0, &big),
        Data::Rejected { abandoned: Some(2) }
    );
}

#[test]
fn a_new_start_ends_an_incomplete_image() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, 100));
    r.data(&mut m, P, 1, 0, &png(40));
    let s = r.start(&mut m, P, start(2, 100));
    assert_eq!(s.abandoned, Some(1));
    assert_eq!(s.placement, Placement::Receiving);
    assert_eq!(r.receiving_bytes(P), 0);
}

#[test]
fn the_end_of_a_command_frees_the_bytes() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, 100));
    r.data(&mut m, P, 1, 0, &png(40));
    assert_eq!(r.end(&mut m, P), Some(1));
    assert_eq!(r.receiving_bytes(P), 0);
    assert_eq!(r.end(&mut m, P), None);
}

#[test]
fn a_bad_length_or_size_hint_shows_the_alt_text_and_begins_nothing() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    for (s, why) in [
        (start(1, 0), AltReason::BadLength),
        (start(1, MAX_IMAGE_BYTES + 1), AltReason::BadLength),
        (
            Start {
                width: 0,
                ..start(1, 10)
            },
            AltReason::InvalidHint,
        ),
        (
            Start {
                width: 65_536,
                ..start(1, 10)
            },
            AltReason::InvalidHint,
        ),
        (
            Start {
                width: 5000,
                height: 5000,
                ..start(1, 10)
            },
            AltReason::InvalidHint,
        ),
    ] {
        assert_eq!(r.start(&mut m, P, s).placement, Placement::Alt(why));
        assert_eq!(r.data(&mut m, P, 1, 0, &png(10)), Data::Dropped);
    }
}

#[test]
fn an_id_in_use_is_refused_and_ssh_is_refused() {
    let (mut m, _) = manager(true);
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, 20));
    r.data(&mut m, P, 1, 0, &png(20));
    let s = r.start(&mut m, P, start(1, 20));
    assert_eq!(s.placement, Placement::Alt(AltReason::DuplicateId));
    let ssh = Producer(2);
    m.register(ssh, Trust::Unsigned { ssh_session: true });
    let s = r.start(&mut m, ssh, start(1, 20));
    assert_eq!(s.placement, Placement::Alt(AltReason::SshSession));
}

#[test]
fn held_bytes_are_capped_per_producer_and_degrade_it() {
    // No decode may start, so finished images wait in the queue holding bytes.
    let limits = Limits {
        in_flight_total: 0,
        ..Limits::default()
    };
    let mut m = ImageManager::new(limits, gate(true), Calls::default(), Sink);
    m.register(P, Trust::Unsigned { ssh_session: false });
    let mut r = Receiver::new();
    let six = 6 * 1024 * 1024;
    let chunk = png(lumen_decoder::frames::MAX_CHUNK);
    for id in 0..5u32 {
        assert_eq!(
            r.start(&mut m, P, start(id, six as u32)).placement,
            Placement::Receiving
        );
        let mut sent = 0usize;
        let mut last = Data::Buffered;
        while sent < six {
            last = r.data(
                &mut m,
                P,
                id,
                sent as u32,
                &chunk[..chunk.len().min(six - sent)],
            );
            sent += chunk.len().min(six - sent);
        }
        assert_eq!(last, Data::Submitted(id, Submit::Queued));
    }
    assert_eq!(m.held_bytes(P), 5 * six as u64);
    assert!(5 * six as u64 + six as u64 > MAX_HELD_BYTES);
    // The sixth would pass the cap: alt text, and the producer is degraded.
    let s = r.start(&mut m, P, start(9, six as u32));
    assert_eq!(s.placement, Placement::Alt(AltReason::HeldBytes));
    assert_eq!(r.data(&mut m, P, 9, 0, &chunk), Data::Dropped);
    let s = r.start(&mut m, P, start(10, 10));
    assert_eq!(s.placement, Placement::Alt(AltReason::Degraded));
    // The next command lifts the degradation.
    assert_eq!(r.end(&mut m, P), None);
    assert_eq!(
        r.start(&mut m, P, start(11, 10)).placement,
        Placement::Receiving
    );
}

#[test]
fn a_rejection_by_the_decoder_is_not_a_crash() {
    struct Declines;
    impl Decode for Declines {
        fn decode(&self, _: Format, _: (u32, u32), _: &[u8]) -> Outcome {
            Outcome::Rejected(Status::Corrupt)
        }
    }
    let mut m = ImageManager::new(Limits::default(), gate(true), Declines, Sink);
    m.register(P, Trust::Unsigned { ssh_session: false });
    let mut r = Receiver::new();
    r.start(&mut m, P, start(1, 20));
    r.data(&mut m, P, 1, 0, &png(20));
    assert_eq!(finished(&mut m), 1);
    assert_eq!(
        r.start(&mut m, P, start(2, 20)).placement,
        Placement::Receiving
    );
}
