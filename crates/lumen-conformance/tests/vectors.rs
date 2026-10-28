//! Runs the JSON vectors in `conformance/vectors` against the reference
//! detector and Track 0 parser.

use lumen_detector::{Detector, Event};
use lumen_track0::{Error, KIND_PARAGRAPH, escaped_text, parse};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Inputs up to this size are split at every position.
const SPLIT_SWEEP_MAX: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
enum Ev {
    Out(Vec<u8>),
    Escaped(Vec<u8>),
    Frame(u8, Vec<u8>),
    Rejected(Vec<u8>),
    Truncated(Vec<u8>),
}

fn load(name: &str) -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../conformance/vectors",
        name,
    ]
    .iter()
    .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        unreachable!("missing vector file {name}");
    };
    let Ok(doc) = serde_json::from_str(&text) else {
        unreachable!("{name} is not valid JSON");
    };
    doc
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map_or(&[], Vec::as_slice)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| unreachable!("bad hex")))
        .collect()
}

/// Bytes of one segment: `{"hex": ".."}` or `{"fill": "<hex byte>", "count": n}`.
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

fn expected(v: &Value) -> Vec<Ev> {
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

/// Run `reads` through a fresh detector, ending the stream afterwards. Adjacent
/// output is joined, so the result does not depend on how the input was split.
fn run(reads: &[&[u8]]) -> Vec<Ev> {
    let mut d = Detector::default();
    let mut events = Vec::new();
    for r in reads {
        d.feed(r, &mut events);
    }
    d.finish(&mut events);
    let mut out: Vec<Ev> = Vec::new();
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

/// Check one vector as written, whole, byte by byte, and (for small inputs) at
/// every single split position.
fn check_detector(file: &str, v: &Value) {
    let id = text(v, "id");
    let want = expected(v);
    let reads = segments(&v["input"]);
    let whole: Vec<u8> = reads.concat();
    let as_written: Vec<&[u8]> = reads.iter().map(Vec::as_slice).collect();
    assert_eq!(run(&as_written), want, "{file}/{id}: reads as written");
    assert_eq!(run(&[&whole]), want, "{file}/{id}: one read");
    let singles: Vec<&[u8]> = whole.chunks(1).collect();
    assert_eq!(run(&singles), want, "{file}/{id}: one byte per read");
    if whole.len() <= SPLIT_SWEEP_MAX {
        for at in 0..=whole.len() {
            let (a, b) = whole.split_at(at);
            assert_eq!(run(&[a, b]), want, "{file}/{id}: split at {at}");
        }
    }
}

#[test]
fn framing_vectors() {
    let doc = load("framing.json");
    let vectors = array(&doc, "vectors");
    assert!(vectors.len() > 10, "framing vectors missing");
    for v in vectors {
        check_detector("framing", v);
    }
}

#[test]
fn limit_vectors_for_the_detector() {
    let doc = load("limits.json");
    for v in array(&doc, "detector_vectors") {
        check_detector("limits", v);
    }
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

#[test]
fn limit_vectors_for_track0() {
    let doc = load("limits.json");
    for v in array(&doc, "track0_vectors") {
        let id = text(v, "id");
        let payload = segments(&v["payload"]).concat();
        let got = parse(kind_of(&v["kind"]), &payload);
        let want = &v["expect"];
        match (&got, want["ok"].as_str(), want["error"].as_str()) {
            (Ok(_), Some("paragraph"), _) => {
                assert_eq!(kind_of(&v["kind"]), KIND_PARAGRAPH, "{id}");
            }
            (Err(e), _, Some(name)) => assert_eq!(error_name(*e), name, "{id}"),
            _ => unreachable!("{id}: got {got:?}, wanted {want}"),
        }
    }
}

#[test]
fn limit_vectors_for_escaped_text() {
    let doc = load("limits.json");
    for v in array(&doc, "escaped_text_vectors") {
        let id = text(v, "id");
        let input = segments(&v["input"]).concat();
        let shown = escaped_text(&input);
        let suffix = text(v, "suffix");
        let max = usize::try_from(v["max_body_bytes"].as_u64().unwrap_or(0)).unwrap_or(0);
        assert!(shown.ends_with(suffix), "{id}: {shown:?}");
        let body = &shown[..shown.len() - suffix.len()];
        assert!(body.len() <= max, "{id}: body is {} bytes", body.len());
        assert!(shown.chars().all(|c| !c.is_control()), "{id}");
    }
}

#[test]
fn listed_limits_have_a_vector_or_a_stated_reason() {
    let doc = load("limits.json");
    let mut ids = BTreeSet::new();
    for key in ["detector_vectors", "track0_vectors", "escaped_text_vectors"] {
        for v in array(&doc, key) {
            assert!(ids.insert(text(v, "id").to_owned()), "duplicate id");
        }
    }
    for v in array(&load("framing.json"), "vectors") {
        ids.insert(text(v, "id").to_owned());
    }
    let mut pending = String::new();
    for row in array(&doc, "table") {
        let id = text(row, "id");
        match text(row, "status") {
            "vector" => {
                let named = array(row, "vectors");
                assert!(!named.is_empty(), "{id}: no vectors named");
                for n in named {
                    let n = n.as_str().unwrap_or_default();
                    assert!(ids.contains(n), "{id}: unknown vector {n}");
                }
            }
            "pending" => {
                assert!(
                    !text(row, "reason").is_empty(),
                    "{id}: pending without a reason"
                );
                let _ = writeln!(pending, "  {id}: {}", text(row, "reason"));
            }
            other => unreachable!("{id}: status {other}"),
        }
    }
    println!("limits without a vector yet:\n{pending}");
}

#[test]
fn grid_vectors_are_well_formed_and_reported_pending() {
    let doc = load("grid-isolation.json");
    assert_eq!(text(&doc, "status"), "pending");
    let mut ids = BTreeSet::new();
    for v in array(&doc, "vectors") {
        assert!(ids.insert(text(v, "id")), "duplicate id");
        assert_eq!(
            segments(&v["input"])
                .iter()
                .map(Vec::len)
                .min()
                .map(|n| n < usize::MAX),
            Some(true),
            "{}: no input",
            text(v, "id")
        );
        assert!(v.get("grid_text").is_some());
    }
    println!(
        "grid isolation: {} vectors not run ({})",
        ids.len(),
        text(&doc, "reason")
    );
}
