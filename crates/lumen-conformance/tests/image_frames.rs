//! Runs `conformance/vectors/image-frames.json`.
//!
//! The expected values were worked out from the image-frame rules, not by
//! running this code and writing down what it happened to do.

use lumen_decoder::frames::{Data, MAX_CHUNK, Placement, Receiver, Start, Started};
use lumen_decoder::images::{
    AltReason, Decode, Gate, ImageId, ImageManager, Limits, Producer, Submit, TextureId,
    TextureSink, Trust, UploadFailed,
};
use lumen_decoder::protocol::Format;
use lumen_decoder::runner::{Image, Outcome};
use lumen_decoder::unlock::{GateRecord, Identities};
use lumen_detector::{Detector, Event};
use lumen_track0::{Block, Error, escaped_text, parse};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SPLIT_SWEEP_MAX: usize = 4096;

fn load() -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../conformance/vectors/image-frames.json",
    ]
    .iter()
    .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        unreachable!("{} is readable", path.display())
    };
    serde_json::from_str(&text).unwrap_or_else(|e| unreachable!("image-frames.json is JSON: {e}"))
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key]
        .as_array()
        .unwrap_or_else(|| unreachable!("{key} is an array"))
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| unreachable!("{key} is a string in {v}"))
}

fn opt_text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn number(v: &Value, key: &str) -> u64 {
    v[key]
        .as_u64()
        .unwrap_or_else(|| unreachable!("{key} is a number in {v}"))
}

fn opt_u32(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .map(|n| u32::try_from(n).unwrap_or_else(|_| unreachable!("{key} fits 32 bits")))
}

fn u32_of(v: &Value, key: &str) -> u32 {
    opt_u32(v, key).unwrap_or_else(|| unreachable!("{key} is a number"))
}

fn flag(v: &Value, key: &str) -> bool {
    v[key]
        .as_bool()
        .unwrap_or_else(|| unreachable!("{key} is a bool"))
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| unreachable!("bad hex")))
        .collect()
}

fn segment(v: &Value) -> Vec<u8> {
    if let Some(h) = v["hex"].as_str() {
        return unhex(h);
    }
    let byte = unhex(text(v, "fill"));
    assert_eq!(byte.len(), 1, "fill is one byte");
    let count = usize::try_from(v["count"].as_u64().unwrap_or(0)).unwrap_or(0);
    vec![byte[0]; count]
}

fn segments(v: &Value) -> Vec<Vec<u8>> {
    v.as_array()
        .map_or_else(Vec::new, |a| a.iter().map(segment).collect())
}

fn kind_of(v: &Value) -> u8 {
    u8::try_from(v.as_u64().unwrap_or(0)).unwrap_or(0)
}

fn error_name(e: Error) -> &'static str {
    match e {
        Error::SealedKind => "sealed_kind",
        Error::UnknownKind => "unknown_kind",
        Error::Malformed => "malformed",
        Error::NotUtf8 => "not_utf8",
        Error::StringTooLong => "string_too_long",
        Error::BadLevel => "bad_level",
        Error::Control => "control",
    }
}

fn reason_name(r: AltReason) -> &'static str {
    match r {
        AltReason::Locked => "locked",
        AltReason::SshSession => "ssh_session",
        AltReason::InvalidHint => "invalid_hint",
        AltReason::QueueFull => "queue_full",
        AltReason::OverBudget => "over_budget",
        AltReason::Degraded => "degraded",
        AltReason::DuplicateId => "duplicate_id",
        AltReason::Rejected(_) => "rejected",
        AltReason::Refused => "refused",
        AltReason::DecoderCrash(_) => "decoder_crash",
        AltReason::TextureBudget => "texture_budget",
        AltReason::UploadFailed => "upload_failed",
        AltReason::BadLength => "bad_length",
        AltReason::HeldBytes => "held_bytes",
        AltReason::OutOfOrder => "out_of_order",
        AltReason::Abandoned => "abandoned",
    }
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::Png => "png",
        Format::Jpeg => "jpeg",
        Format::Webp => "webp",
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Ev {
    Out(Vec<u8>),
    Escaped(Vec<u8>),
    Frame(u8, Vec<u8>),
    Rejected(Vec<u8>),
    Truncated(Vec<u8>),
}

fn expected_events(v: &Value) -> Vec<Ev> {
    let mut out = Vec::new();
    for e in array(v, "events") {
        out.push(if e.get("output").is_some() {
            Ev::Out(segment(&e["output"]))
        } else if e.get("escaped").is_some() {
            Ev::Escaped(segment(&e["escaped"]))
        } else if e.get("frame").is_some() {
            Ev::Frame(
                kind_of(&e["frame"]["kind"]),
                segment(&e["frame"]["payload"]),
            )
        } else if e.get("rejected").is_some() {
            Ev::Rejected(segment(&e["rejected"]))
        } else {
            Ev::Truncated(segment(&e["truncated"]))
        });
    }
    out
}

fn run_detector(reads: &[&[u8]]) -> Vec<Ev> {
    let mut d = Detector::default();
    let mut events = Vec::new();
    for r in reads {
        d.feed(r, &mut events);
    }
    d.finish(&mut events);
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::Output(b) => match out.last_mut() {
                Some(Ev::Out(prev)) => prev.extend_from_slice(&b),
                _ => out.push(Ev::Out(b)),
            },
            Event::Escaped(b) => match out.last_mut() {
                Some(Ev::Escaped(prev)) => prev.extend_from_slice(&b),
                _ => out.push(Ev::Escaped(b)),
            },
            Event::Frame { kind, payload } => out.push(Ev::Frame(kind, payload)),
            Event::Rejected(b) => out.push(Ev::Rejected(b)),
            Event::Truncated(b) => out.push(Ev::Truncated(b)),
        }
    }
    out
}

fn check_detector(id: &str, input: &[Vec<u8>], want: &[Ev]) {
    let whole: Vec<u8> = input.iter().flatten().copied().collect();
    let as_written: Vec<&[u8]> = input.iter().map(Vec::as_slice).collect();
    assert_eq!(run_detector(&as_written), want, "{id}: reads as written");
    assert_eq!(run_detector(&[&whole]), want, "{id}: one read");
    let singles: Vec<&[u8]> = whole.chunks(1).collect();
    assert_eq!(run_detector(&singles), want, "{id}: one byte per read");
    if whole.len() <= SPLIT_SWEEP_MAX {
        for at in 0..=whole.len() {
            let (a, b) = whole.split_at(at);
            assert_eq!(run_detector(&[a, b]), want, "{id}: split at {at}");
        }
    }
}

fn check_block(id: &str, block: &Block, expect: &Value) {
    match (block, text(expect, "ok")) {
        (
            Block::ImageStart {
                id: got_id,
                format,
                width,
                height,
                length,
                alt,
            },
            "image_start",
        ) => {
            assert_eq!(*got_id, u32_of(expect, "id"), "{id}");
            assert_eq!(u64::from(*format), number(expect, "format"), "{id}");
            assert_eq!(*width, u32_of(expect, "width"), "{id}");
            assert_eq!(*height, u32_of(expect, "height"), "{id}");
            assert_eq!(*length, u32_of(expect, "length"), "{id}");
            assert_eq!(alt, text(expect, "alt"), "{id}");
        }
        (
            Block::ImageData {
                id: got_id,
                offset,
                data,
            },
            "image_data",
        ) => {
            assert_eq!(*got_id, u32_of(expect, "id"), "{id}");
            assert_eq!(*offset, u32_of(expect, "offset"), "{id}");
            assert_eq!(
                data.len(),
                usize::try_from(number(expect, "len")).unwrap_or(0),
                "{id}"
            );
            if let Some(prefix) = opt_text(expect, "prefix_hex") {
                let prefix = unhex(prefix);
                assert!(data.starts_with(&prefix), "{id}");
            }
        }
        other => unreachable!("{id}: {other:?} against {expect}"),
    }
}

#[test]
fn parse_vectors() {
    let doc = load();
    let vectors = array(&doc, "parse");
    assert!(vectors.len() > 20, "parse vectors missing");
    for v in vectors {
        let id = text(v, "id");
        let payload = segments(&v["payload"]).concat();
        let got = parse(kind_of(&v["kind"]), &payload);
        let expect = &v["expect"];
        if let Some(name) = opt_text(expect, "error") {
            let Err(e) = got else {
                unreachable!("{id}: expected {name}, parsed {got:?}")
            };
            assert_eq!(error_name(e), name, "{id}");
        } else {
            let Ok(block) = got else {
                unreachable!("{id}: expected a block, got {got:?}")
            };
            check_block(id, &block, expect);
        }
    }
}

#[test]
fn detector_vectors() {
    let doc = load();
    let vectors = array(&doc, "detector");
    assert!(vectors.len() > 5, "detector vectors missing");
    for v in vectors {
        check_detector(text(v, "id"), &segments(&v["input"]), &expected_events(v));
    }
}

#[test]
fn escaped_vectors() {
    let doc = load();
    for v in array(&doc, "escaped") {
        let id = text(v, "id");
        let shown = escaped_text(&segments(&v["input"]).concat());
        let suffix = text(v, "suffix");
        assert!(shown.ends_with(suffix), "{id}: {shown}");
        let body = &shown[..shown.len() - suffix.len()];
        assert_eq!(body, text(v, "body"), "{id}");
        assert!(shown.chars().all(|c| !c.is_control()), "{id}");
    }
}

#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<(Format, usize)>>>);

impl Calls {
    fn all(&self) -> Vec<(Format, usize)> {
        self.0.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Decode for Calls {
    fn decode(&self, format: Format, _hint: (u32, u32), bytes: &[u8]) -> Outcome {
        if let Ok(mut c) = self.0.lock() {
            c.push((format, bytes.len()));
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

struct Harness {
    receiver: Receiver,
    manager: ImageManager<Sink>,
    calls: Calls,
    in_flight: usize,
    /// Decodes this harness has been told to wait for.
    decodes: usize,
}

fn gate(open: bool) -> Gate {
    let identities = Identities {
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
        identities,
        records,
    }
}

fn harness_for(v: &Value) -> Harness {
    let open = text(v, "gate") == "open";
    let in_flight = usize::try_from(v["in_flight_total"].as_u64().unwrap_or(2)).unwrap_or(2);
    let limits = Limits {
        in_flight_total: in_flight,
        ..Limits::default()
    };
    let calls = Calls::default();
    let mut manager = ImageManager::new(limits, gate(open), calls.clone(), Sink);
    let producers = v["producers"].as_array();
    if let Some(list) = producers {
        for p in list {
            if flag(p, "register") {
                manager.register(
                    Producer(number(p, "id")),
                    Trust::Unsigned {
                        ssh_session: flag(p, "ssh"),
                    },
                );
            }
        }
    } else if v["register"].as_bool().unwrap_or(true) {
        manager.register(
            Producer(1),
            Trust::Unsigned {
                ssh_session: v["ssh"].as_bool().unwrap_or(false),
            },
        );
    }
    Harness {
        receiver: Receiver::new(),
        manager,
        calls,
        in_flight,
        decodes: 0,
    }
}

fn producer_of(step: &Value) -> Producer {
    Producer(step["producer"].as_u64().unwrap_or(1))
}

fn bytes_of(step: &Value) -> Vec<u8> {
    if step.get("hex").is_some() {
        return unhex(text(step, "hex"));
    }
    let mut out = unhex(step["prefix_hex"].as_str().unwrap_or(""));
    let count = usize::try_from(number(step, "count")).unwrap_or(0);
    let fill = unhex(text(step, "fill"));
    assert_eq!(fill.len(), 1, "fill is one byte");
    out.resize(out.len() + count, fill[0]);
    out
}

fn expect_decode(h: &mut Harness) {
    if h.in_flight == 0 {
        return;
    }
    h.decodes += 1;
    let target = h.decodes;
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = h.manager.poll();
        if h.calls.all().len() >= target {
            return;
        }
        assert!(Instant::now() < end, "decode did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn check_submit(id: &str, h: &mut Harness, submit: &Submit, expect: &Value) {
    match (submit, text(expect, "submit")) {
        (Submit::Queued, "queued") => {
            expect_decode(h);
            if h.in_flight > 0 {
                let want_len = usize::try_from(number(expect, "bytes")).unwrap_or(0);
                let want_fmt = text(expect, "format");
                let calls = h.calls.all();
                assert!(
                    calls
                        .iter()
                        .any(|(format, len)| format_name(*format) == want_fmt && *len == want_len),
                    "{id}: {calls:?}"
                );
            }
        }
        (Submit::AltText(reason), "alt") => {
            assert_eq!(reason_name(*reason), text(expect, "reason"), "{id}");
        }
        other => unreachable!("{id}: submit {other:?}"),
    }
}

fn check_data(id: &str, h: &mut Harness, got: &Data, expect: &Value) {
    match (got, text(expect, "result")) {
        (Data::Buffered, "buffered") | (Data::Dropped, "dropped") => {}
        (Data::Rejected { abandoned }, "rejected") => {
            assert!(expect.get("abandoned").is_some(), "{id}");
            assert_eq!(*abandoned, opt_u32(expect, "abandoned"), "{id}");
        }
        (Data::Submitted(image, submit), "submitted") => {
            assert_eq!(*image, u32_of(expect, "id"), "{id}");
            check_submit(id, h, submit, expect);
        }
        other => unreachable!("{id}: data {other:?}"),
    }
}

fn check_start(id: &str, got: &Started, expect: &Value) {
    assert!(expect.get("abandoned").is_some(), "{id}");
    assert_eq!(got.abandoned, opt_u32(expect, "abandoned"), "{id}");
    match (&got.placement, text(expect, "placement")) {
        (Placement::Receiving, "receiving") => {}
        (Placement::Alt(reason), "alt") => {
            assert_eq!(reason_name(*reason), text(expect, "reason"), "{id}");
        }
        other => unreachable!("{id}: start {other:?}"),
    }
}

fn one_image(h: &mut Harness, producer: Producer, id: ImageId, length: usize, prefix: &[u8]) {
    let started = h.receiver.start(
        &mut h.manager,
        producer,
        Start {
            id,
            width: 1,
            height: 1,
            length: u32::try_from(length).unwrap_or(0),
        },
    );
    assert_eq!(started.placement, Placement::Receiving, "repeat {id}");
    let mut body = vec![0u8; length];
    let n = prefix.len().min(length);
    body[..n].copy_from_slice(&prefix[..n]);
    let got = h.receiver.data(&mut h.manager, producer, id, 0, &body);
    let Data::Submitted(_, Submit::Queued) = got else {
        unreachable!("repeat {id} queued, got {got:?}")
    };
    expect_decode(h);
}

fn fill_image(id: &str, h: &mut Harness, step: &Value) {
    let producer = producer_of(step);
    let image = u32_of(step, "id");
    let total = usize::try_from(number(step, "total")).unwrap_or(0);
    let chunk = usize::try_from(number(step, "chunk")).unwrap_or(0);
    assert!(chunk > 0 && chunk <= MAX_CHUNK, "{id}: chunk");
    let prefix = unhex(text(step, "prefix_hex"));
    assert!(prefix.len() <= total, "{id}: prefix");
    let mut body = vec![0u8; total];
    body[..prefix.len()].copy_from_slice(&prefix);
    let mut off = 0usize;
    while off < total {
        let n = chunk.min(total - off);
        let Ok(offset) = u32::try_from(off) else {
            unreachable!("{id}: offset fits 32 bits")
        };
        let got = h
            .receiver
            .data(&mut h.manager, producer, image, offset, &body[off..off + n]);
        off += n;
        if off < total {
            assert_eq!(got, Data::Buffered, "{id} at {off}");
        } else {
            check_data(id, h, &got, &step["expect"]);
        }
    }
}

fn run_steps(id: &str, h: &mut Harness, steps: &[Value]) {
    for step in steps {
        let op = text(step, "op");
        let producer = producer_of(step);
        match op {
            "start" => {
                let got = h.receiver.start(
                    &mut h.manager,
                    producer,
                    Start {
                        id: u32_of(step, "id"),
                        width: u32_of(step, "width"),
                        height: u32_of(step, "height"),
                        length: u32_of(step, "length"),
                    },
                );
                check_start(id, &got, &step["expect"]);
            }
            "data" => {
                let body = bytes_of(step);
                let got = h.receiver.data(
                    &mut h.manager,
                    producer,
                    u32_of(step, "id"),
                    u32_of(step, "offset"),
                    &body,
                );
                check_data(id, h, &got, &step["expect"]);
            }
            "fill" => fill_image(id, h, step),
            "repeat" => {
                let from = u32_of(step, "from");
                let count = u32_of(step, "count");
                let length = usize::try_from(number(step, "length")).unwrap_or(0);
                let prefix = unhex(text(step, "prefix_hex"));
                for n in 0..count {
                    let Some(image) = from.checked_add(n) else {
                        unreachable!("{id}: id")
                    };
                    one_image(h, producer, image, length, &prefix);
                }
            }
            "end" => {
                let got = h.receiver.end(&mut h.manager, producer);
                assert_eq!(got, opt_u32(&step["expect"], "abandoned"), "{id}");
            }
            "held" => {
                assert_eq!(
                    h.manager.held_bytes(producer),
                    number(step, "bytes"),
                    "{id}"
                );
            }
            "receiving" => {
                assert_eq!(
                    h.receiver.receiving_bytes(producer),
                    number(step, "bytes"),
                    "{id}"
                );
            }
            other => unreachable!("{id}: op {other}"),
        }
    }
}

#[test]
fn reassembly_vectors() {
    let doc = load();
    let vectors = array(&doc, "reassembly");
    assert!(vectors.len() > 15, "reassembly vectors missing");
    for v in vectors {
        let id = text(v, "id");
        let mut h = harness_for(v);
        run_steps(id, &mut h, array(v, "steps"));
    }
}

fn apply_frames(h: &mut Harness, events: &[Ev]) -> Option<(ImageId, Submit)> {
    let mut last = None;
    for e in events {
        let Ev::Frame(kind, payload) = e else {
            continue;
        };
        match parse(*kind, payload) {
            Ok(Block::ImageStart {
                id,
                width,
                height,
                length,
                ..
            }) => {
                let _ = h.receiver.start(
                    &mut h.manager,
                    Producer(1),
                    Start {
                        id,
                        width,
                        height,
                        length,
                    },
                );
            }
            Ok(Block::ImageData { id, offset, data }) => {
                if let Data::Submitted(id, submit) =
                    h.receiver
                        .data(&mut h.manager, Producer(1), id, offset, &data)
                {
                    last = Some((id, submit));
                }
            }
            _ => {}
        }
    }
    last
}

fn check_outcome(id: &str, h: &mut Harness, last: Option<&(ImageId, Submit)>, outcome: &Value) {
    assert_eq!(
        h.receiver.receiving_bytes(Producer(1)),
        number(outcome, "receiving"),
        "{id}"
    );
    match (last, outcome.get("submit")) {
        (_, None) | (None, Some(Value::Null)) => {}
        (Some((image, submit)), Some(expect)) => {
            assert_eq!(*image, u32_of(expect, "id"), "{id}");
            check_submit(id, h, submit, expect);
        }
        other => unreachable!("{id}: outcome {other:?}"),
    }
}

#[test]
fn wire_vectors() {
    let doc = load();
    let vectors = array(&doc, "wire");
    assert!(vectors.len() > 3, "wire vectors missing");
    for v in vectors {
        let id = text(v, "id");
        let input = segments(&v["input"]);
        if v.get("events").is_some() {
            check_detector(id, &input, &expected_events(v));
        }
        let whole: Vec<u8> = input.iter().flatten().copied().collect();
        let mut h = harness_for(v);
        let events = run_detector(&[&whole]);
        let last = apply_frames(&mut h, &events);
        check_outcome(id, &mut h, last.as_ref(), &v["outcome"]);

        // The same bytes, split into the reads the vector wrote.
        let mut h2 = harness_for(v);
        let as_written: Vec<&[u8]> = input.iter().map(Vec::as_slice).collect();
        let events = run_detector(&as_written);
        let last = apply_frames(&mut h2, &events);
        check_outcome(id, &mut h2, last.as_ref(), &v["outcome"]);
    }
}
