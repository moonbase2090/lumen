//! Runs `conformance/vectors/images.json` against the image unlock rule and the
//! host-side check of a decoder reply.
//!
//! The vectors say what the terminal must do; their expected values were worked
//! out from the spec, not by running this code.

use lumen_decoder::protocol::{Reply, Status};
use lumen_decoder::runner::{Crash, Outcome, validate};
use lumen_decoder::unlock::{GateRecord, Identities, images_enabled};
use serde_json::Value;
use std::path::PathBuf;

fn load() -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../conformance/vectors/images.json",
    ]
    .iter()
    .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        unreachable!("{} is readable", path.display())
    };
    serde_json::from_str(&text).unwrap_or_else(|e| unreachable!("images.json is JSON: {e}"))
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

fn flag(v: &Value, key: &str) -> bool {
    v[key]
        .as_bool()
        .unwrap_or_else(|| unreachable!("{key} is a bool in {v}"))
}

fn number(v: &Value, key: &str) -> u64 {
    v[key]
        .as_u64()
        .unwrap_or_else(|| unreachable!("{key} is a number in {v}"))
}

fn small(v: &Value, key: &str) -> u32 {
    u32::try_from(number(v, key)).unwrap_or_else(|_| unreachable!("{key} fits 32 bits in {v}"))
}

#[test]
fn unlock_vectors() {
    let doc = load();
    let vectors = array(&doc, "unlock_vectors");
    assert_ne!(vectors.len(), 0);
    for v in vectors {
        let id = text(v, "id");
        let records: Vec<GateRecord> = array(v, "records")
            .iter()
            .map(|r| GateRecord {
                platform: text(r, "platform").to_owned(),
                decoder_build: text(r, "decoder_build").to_owned(),
                image_path_build: text(r, "image_path_build").to_owned(),
                decoder_gate_passed: flag(r, "decoder_gate_passed"),
                memory_gate_passed: flag(r, "memory_gate_passed"),
            })
            .collect();
        let current = Identities {
            decoder_build: text(v, "decoder_build").to_owned(),
            image_path_build: text(v, "image_path_build").to_owned(),
        };
        let shown = images_enabled(
            flag(v, "setting_on"),
            text(v, "platform"),
            &current,
            &records,
        );
        let want = match text(v, "expect") {
            "images" => true,
            "alt_text" => false,
            other => unreachable!("unknown expectation {other} in {id}"),
        };
        assert_eq!(shown, want, "{id}");
    }
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Ok => "ok",
        Status::Malformed => "malformed",
        Status::Unsupported => "unsupported",
        Status::TooLarge => "too_large",
        Status::Corrupt => "corrupt",
        Status::Internal => "internal",
    }
}

fn crash_name(c: Crash) -> &'static str {
    match c {
        Crash::Spawn => "spawn",
        Crash::Timeout => "timeout",
        Crash::Exit => "exit",
        Crash::Overflow => "overflow",
        Crash::Short => "short",
        Crash::BadReply => "bad_reply",
    }
}

#[test]
fn reply_vectors() {
    let doc = load();
    let vectors = array(&doc, "reply_vectors");
    assert_ne!(vectors.len(), 0);
    for v in vectors {
        let id = text(v, "id");
        let r = &v["reply"];
        let status = u8::try_from(number(r, "status"))
            .unwrap_or_else(|_| unreachable!("status is a byte in {id}"));
        let header = Reply {
            status,
            width: small(r, "width"),
            height: small(r, "height"),
            stride: small(r, "stride"),
            data_len: number(r, "data_len"),
        };
        let body = usize::try_from(number(v, "body_bytes"))
            .unwrap_or_else(|_| unreachable!("body fits memory in {id}"));
        let mut out = header.encode().to_vec();
        out.extend(std::iter::repeat_n(0x80_u8, body));
        let got = validate(
            &out,
            flag(v, "exited_ok"),
            number(v, "reserved"),
            number(v, "cap"),
        );
        let expect = &v["expect"];
        if let Some(dim) = expect.get("image") {
            let dim = dim
                .as_array()
                .unwrap_or_else(|| unreachable!("image is [w, h] in {id}"));
            let Outcome::Image(img) = got else {
                unreachable!("{id}: expected an image, got {got:?}")
            };
            assert_eq!(
                (u64::from(img.width), u64::from(img.height)),
                (dim[0].as_u64().unwrap_or(0), dim[1].as_u64().unwrap_or(0)),
                "{id}"
            );
        } else if let Some(name) = expect.get("crash").and_then(Value::as_str) {
            let Outcome::Crash(c) = got else {
                unreachable!("{id}: expected a crash, got {got:?}")
            };
            assert_eq!(crash_name(c), name, "{id}");
        } else if let Some(name) = expect.get("rejected").and_then(Value::as_str) {
            let Outcome::Rejected(s) = got else {
                unreachable!("{id}: expected a decline, got {got:?}")
            };
            assert_eq!(status_name(s), name, "{id}");
        } else {
            unreachable!("{id}: unknown expectation {expect}");
        }
    }
}
