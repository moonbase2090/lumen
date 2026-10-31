//! Runs `conformance/vectors/budgets.json` against the budget model.
//!
//! The vectors say what the model must do; their expected values were worked out
//! from the spec's numbers, not by running this code.

use lumen_budget::item::{self, ImageRefused};
use lumen_budget::{
    Claimant, Denied, DocumentRates, Ledger, ProducerRates, Resource, Ticket, limits,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn load() -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../conformance/vectors/budgets.json",
    ]
    .iter()
    .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        unreachable!("{} is readable", path.display())
    };
    serde_json::from_str(&text).unwrap_or_else(|e| unreachable!("budgets.json is JSON: {e}"))
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

/// A number, or a decimal string for what a JSON number cannot hold exactly.
fn number(v: &Value, key: &str) -> u64 {
    match &v[key] {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
    .unwrap_or_else(|| unreachable!("{key} is a number in {v}"))
}

fn who(s: &str) -> (Claimant, u32) {
    if s == "local" {
        return (Claimant::Local, 0);
    }
    let (kind, id) = s
        .split_once(':')
        .unwrap_or_else(|| unreachable!("a producer is kind:id: {s}"));
    let id: u32 = id
        .parse()
        .unwrap_or_else(|_| unreachable!("a producer id is a number: {s}"));
    match kind {
        "signed" => (Claimant::Producer { id, sealed: true }, id),
        "unsigned" => (Claimant::Producer { id, sealed: false }, id),
        _ => unreachable!("unknown producer kind {kind}"),
    }
}

fn denial(d: Denied) -> &'static str {
    match d {
        Denied::OverCapacity => "over_capacity",
        Denied::LocalReserve => "local_reserve",
        Denied::ProducerShare => "producer_share",
        Denied::UnsignedShare => "unsigned_share",
    }
}

#[test]
fn ledger_vectors() {
    let doc = load();
    let mut ids = BTreeSet::new();
    for v in array(&doc, "ledger_vectors") {
        let id = text(v, "id");
        assert!(ids.insert(id), "duplicate id {id}");
        let mut ledger = Ledger::new();
        if let Some(panes) = v["panes"].as_u64() {
            ledger.set_visible_panes(u32::try_from(panes).unwrap_or(1));
        }
        let mut held: BTreeMap<String, Ticket> = BTreeMap::new();
        for (n, op) in array(v, "ops").iter().enumerate() {
            let at = format!("{id}, op {n}");
            match text(op, "op") {
                "reserve" => {
                    let resource = Resource::from_name(text(op, "resource"))
                        .unwrap_or_else(|| unreachable!("{at}: unknown resource"));
                    let (claimant, _) = who(text(op, "who"));
                    let got = ledger.reserve(resource, claimant, number(op, "amount"));
                    match (&got, text(op, "expect")) {
                        (Ok(_), "ok") => {}
                        (Err(d), want) if want == denial(*d) => {}
                        _ => unreachable!("{at}: got {got:?}, wanted {}", op["expect"]),
                    }
                    if let (Ok(ticket), Some(tag)) = (got, op["tag"].as_str()) {
                        assert!(held.insert(tag.to_owned(), ticket).is_none(), "{at}: tag");
                    }
                }
                "release" => {
                    let Some(ticket) = held.remove(text(op, "tag")) else {
                        unreachable!("{at}: no ticket with that tag")
                    };
                    assert!(ledger.release(ticket).is_ok(), "{at}");
                }
                "used" => {
                    let resource = Resource::from_name(text(op, "resource"))
                        .unwrap_or_else(|| unreachable!("{at}: unknown resource"));
                    assert_eq!(ledger.used(resource), number(op, "expect"), "{at}");
                }
                other => unreachable!("{at}: unknown op {other}"),
            }
        }
    }
    assert!(ids.len() >= 40, "{} ledger vectors", ids.len());
}

#[test]
fn rate_vectors() {
    let doc = load();
    for v in array(&doc, "rate_vectors") {
        let id = text(v, "id");
        let panes = u32::try_from(number(v, "panes")).unwrap_or(1);
        let mut producers: BTreeMap<String, ProducerRates> = BTreeMap::new();
        let mut patches = DocumentRates::new();
        for (n, op) in array(v, "ops").iter().enumerate() {
            let at = format!("{id}, op {n}");
            let count = v_count(op);
            let want = op["expect"]
                .as_bool()
                .unwrap_or_else(|| unreachable!("{at}: expect is a bool"));
            for i in 0..count {
                let now = number(op, "at_ms");
                let got = match text(op, "op") {
                    "frame" => {
                        let who_text = text(op, "who");
                        let (claimant, _) = who(who_text);
                        let sealed = matches!(claimant, Claimant::Producer { sealed: true, .. });
                        producers
                            .entry(who_text.to_owned())
                            .or_insert_with(|| ProducerRates::new(sealed, panes))
                            .admit_frame(now, number(op, "bytes"))
                    }
                    "patch" => patches.admit_patches(now, number(op, "block"), number(op, "n")),
                    other => unreachable!("{at}: unknown op {other}"),
                };
                assert_eq!(got, want, "{at}, repeat {i}");
            }
        }
    }
}

fn v_count(op: &Value) -> u64 {
    op["count"].as_u64().unwrap_or(1)
}

#[test]
fn item_vectors() {
    let doc = load();
    for v in array(&doc, "item_vectors") {
        let id = text(v, "id");
        let want = &v["expect"];
        match text(v, "check") {
            "frame_size" => {
                let track_b = v["images_track_b"].as_bool().unwrap_or(false);
                let got = item::frame_size_ok(number(v, "len"), track_b);
                assert_eq!(Some(got), want.as_bool(), "{id}");
            }
            "nesting" => {
                assert_eq!(
                    Some(item::nesting_ok(number(v, "depth"))),
                    want.as_bool(),
                    "{id}"
                );
            }
            "table" => {
                let got = item::table_ok(number(v, "columns"), number(v, "cells"));
                assert_eq!(Some(got), want.as_bool(), "{id}");
            }
            "grid" => {
                let got = item::grid_size_ok(number(v, "columns"), number(v, "rows"));
                assert_eq!(Some(got), want.as_bool(), "{id}");
            }
            "layers" => {
                assert_eq!(
                    Some(item::layers_ok(number(v, "layers"))),
                    want.as_bool(),
                    "{id}"
                );
            }
            "image" => {
                let got = item::image_reservation(number(v, "width"), number(v, "height"));
                match (got, want["ok"].as_object(), want["refused"].as_str()) {
                    (Ok(r), Some(ok), _) => {
                        assert_eq!(Some(r.bytes), ok["bytes"].as_u64(), "{id}");
                        assert_eq!(Some(r.read_limit), ok["read_limit"].as_u64(), "{id}");
                    }
                    (Err(ImageRefused::Side), _, Some("side"))
                    | (Err(ImageRefused::Pixels), _, Some("pixels")) => {}
                    (got, _, _) => unreachable!("{id}: got {got:?}, wanted {want}"),
                }
            }
            other => unreachable!("{id}: unknown check {other}"),
        }
    }
}

/// The table in the spec, written out here once more: every number the model
/// holds is the number in the document, and a change to either is a failure.
#[test]
fn the_limits_are_the_numbers_of_the_spec_table() {
    const MIB: u64 = 1 << 20;
    assert_eq!(limits::BLOCKS_PER_DOCUMENT, 100_000);
    assert_eq!(limits::PATCHES_PER_BLOCK_PER_SEC, 10_000);
    assert_eq!(limits::PATCHES_PER_DOCUMENT_PER_SEC, 50_000);
    assert_eq!(limits::SHAPING_CACHE_BYTES, 64 * MIB);
    assert_eq!((limits::TABLE_COLUMNS, limits::TABLE_CELLS), (256, 100_000));
    assert_eq!(limits::GRIDS_PER_DOCUMENT, 64);
    assert_eq!(limits::GRID_CELLS_PER_DOCUMENT, 4_000_000);
    assert_eq!(limits::SCROLLBACK_BYTES, 64 * MIB);
    assert_eq!((limits::GRID_COLUMNS, limits::GRID_ROWS), (1000, 500));
    assert_eq!(limits::FRAMES_PER_PRODUCER_PER_SEC, 2_000);
    assert_eq!(limits::BYTES_PER_PRODUCER_PER_SEC, 8 * MIB);
    assert_eq!(limits::FRAME_BYTES, MIB);
    assert_eq!(limits::IMAGE_FRAME_BYTES_TRACK_B, 16 * MIB);
    assert_eq!(limits::NESTING_DEPTH, 32);
    assert_eq!(limits::DECODED_IMAGE_BYTES, 256 * MIB);
    assert_eq!(limits::GPU_TEXTURE_BYTES, 512 * MIB);
    assert_eq!(limits::LAYERS, 8);
    // And the model's capacities are those numbers.
    for (r, want) in [
        (Resource::Blocks, 100_000),
        (Resource::Grids, 64),
        (Resource::GridCells, 4_000_000),
        (Resource::ScrollbackBytes, 64 * MIB),
        (Resource::ShapingCacheBytes, 64 * MIB),
        (Resource::DecodedImageBytes, 256 * MIB),
        (Resource::GpuTextureBytes, 512 * MIB),
    ] {
        assert_eq!(r.capacity(), want, "{r:?}");
    }
}

/// The vectors name exactly the resources the model has.
#[test]
fn every_resource_has_its_own_vectors() {
    let doc = load();
    let text_of: BTreeSet<String> = array(&doc, "ledger_vectors")
        .iter()
        .map(|v| text(v, "id").to_owned())
        .collect();
    for r in Resource::ALL {
        for suffix in [
            "local-fills-the-whole-budget",
            "one-producer-holds-half-of-the-remainder",
            "producers-cannot-reach-the-local-reserve",
            "unsigned-producers-share-a-quarter",
            "the-unsigned-quarter-is-split-across-four-panes",
            "release-gives-the-share-back",
        ] {
            let id = format!("{}-{suffix}", r.name());
            assert!(text_of.contains(&id), "no vector {id}");
        }
    }
}
