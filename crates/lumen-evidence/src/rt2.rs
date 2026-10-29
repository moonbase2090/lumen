//! The `rt-2` corpus: real sealed frames, built by the sealing code itself.
//!
//! Every frame is sealed under the fixed test keys the driver gives each
//! replayed layer ([`crate::TEST_ROOT`], [`crate::TEST_LAYER`],
//! [`crate::TEST_CAP`]), so the sealed upgrade path is exercised and
//! `sealed_opened` is above zero. The committed JSON file is the output of
//! [`rt2_json`]; a test fails if the two differ.

use crate::{TEST_CAP, TEST_LAYER, TEST_ROOT};
use lumen_detector::TAG;
use lumen_sealed::{Direction, DirectionKeys, HEADER_LEN, LayerRoot, Sender};

use serde_json::{Value, json};
use std::fmt::Write as _;

/// The frame type of every test frame: a sealed type, below the unsealed range.
const SEALED_KIND: u8 = 0x01;

/// The corpus revision.
pub const REVISION: &str = "rt-2";

fn sender(root: [u8; 32], first_seq: u64) -> Sender {
    let keys = DirectionKeys::derive(&LayerRoot::new(root), TEST_LAYER, Direction::ToTerminal);
    Sender::new(keys, TEST_LAYER, Direction::ToTerminal, first_seq)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The payload: a two-byte text length, then the text. The layer reports a
/// sealed payload as opaque bytes, so only its length is observable.
fn paragraph(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

/// The detector frame around a sealed frame (introducer, tag, type, length).
fn wrap(sealed: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(SEALED_KIND);
    v.extend_from_slice(&u32::try_from(sealed.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(sealed);
    v
}

/// The next frame from `s`, sealed for the test capability.
fn next(s: &mut Sender, text: &str) -> Vec<u8> {
    match s.seal(SEALED_KIND, TEST_CAP, &paragraph(text)) {
        Ok(sealed) => wrap(&sealed),
        Err(e) => unreachable!("sealing a short payload succeeds: {e:?}"),
    }
}

/// The same frame with one body byte changed: the header verifies and the
/// AEAD check fails.
fn tamper(mut frame: Vec<u8>) -> Vec<u8> {
    frame[2 + TAG.len() + 1 + 4 + HEADER_LEN] ^= 1;
    frame
}

fn read(bytes: &[u8]) -> Value {
    json!({ "hex": hex(bytes) })
}

fn case(id: &str, category: &str, reads: &[Value], forbid: &[&str]) -> Value {
    let mut c = json!({ "id": id, "category": category, "reads": reads });
    if !forbid.is_empty() {
        c["forbid"] = json!(forbid);
    }
    c
}

/// The corpus as pretty-printed JSON with a trailing newline.
#[must_use]
pub fn rt2_json() -> String {
    let mut cases = Vec::new();

    let f = next(&mut sender(TEST_ROOT, 1), "valid sealed frame");
    cases.push(case("sealed-valid", "valid", &[read(&f)], &[]));

    let mut s = sender(TEST_ROOT, 1);
    let (a, b) = (next(&mut s, "first"), next(&mut s, "second"));
    cases.push(case(
        "sealed-valid-two-in-order",
        "valid",
        &[read(&a), read(&b)],
        &[],
    ));

    let f = next(&mut sender(TEST_ROOT, 1), "split across reads");
    let (head, tail) = f.split_at(f.len() / 2);
    cases.push(case(
        "sealed-valid-split-across-reads",
        "valid",
        &[read(head), read(tail)],
        &[],
    ));

    let f = tamper(next(&mut sender(TEST_ROOT, 1), "tampered body"));
    cases.push(case(
        "sealed-tampered-body",
        "tampered",
        &[read(&f)],
        &["sealed"],
    ));

    let mut s = sender(TEST_ROOT, 1);
    let a = next(&mut s, "replayed");
    cases.push(case(
        "sealed-replayed-frame",
        "replayed",
        &[read(&a), read(&a)],
        &[],
    ));

    let f = next(&mut sender([5; 32], 1), "other keys");
    cases.push(case(
        "sealed-wrong-keys",
        "forged",
        &[read(&f)],
        &["sealed"],
    ));

    let mut s = sender(TEST_ROOT, 1);
    let many: Vec<u8> = (0..300)
        .flat_map(|i| next(&mut s, &format!("p{i}")))
        .collect();
    cases.push(case(
        "sealed-over-body-cap-in-one-second",
        "over-cap",
        &[read(&many)],
        &[],
    ));

    let mut s = sender(TEST_ROOT, 1);
    let mut reads: Vec<Value> = (0..5)
        .map(|i| read(&tamper(next(&mut s, &format!("abort{i}")))))
        .collect();
    reads.push(read(&next(&mut s, "after the pin")));
    cases.push(case("sealed-aborts-pin-the-layer", "abort", &reads, &[]));

    let f = next(&mut sender(TEST_ROOT, 1), "cut mid body");
    cases.push(case(
        "sealed-abort-mid-body",
        "abort",
        &[read(&f[..f.len() - 4])],
        &["sealed"],
    ));

    let mut out = serde_json::to_string_pretty(&json!({ "revision": REVISION, "cases": cases }))
        .unwrap_or_default();
    out.push('\n');
    out
}
