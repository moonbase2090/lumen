//! The `pc-1` pane corpus: scripts for the multiplexer placement comparison.
//!
//! The committed JSON file is the output of [`pc1_json`]; a test fails if the
//! two differ. Pane output is real Track 0 framing, built here with the same
//! framing the arms read, so no hex is typed by hand.

use lumen_detector::TAG;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The corpus revision.
pub const REVISION: &str = "pc-1";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// A Track 0 frame of `kind`: introducer, tag, type, length, payload.
fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn paragraph(text: &str) -> Vec<u8> {
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    frame(0x80, &body)
}

fn pane(id: u32, x: i64, y: i64, w: u64, h: u64) -> Value {
    json!({"id": id, "x": x, "y": y, "w": w, "h": h})
}

fn layout(generation: u32, panes: &[Value]) -> Value {
    json!({"op": "layout", "generation": generation, "panes": panes})
}

fn output(pane: u32, bytes: &[u8]) -> Value {
    json!({"op": "output", "pane": pane, "hex": hex(bytes)})
}

fn case(id: &str, category: &str, script: &[Value], forbid: &[&str]) -> Value {
    let mut c = json!({
        "id": id, "category": category,
        "screen": {"cols": 80, "rows": 24}, "script": script
    });
    if !forbid.is_empty() {
        c["forbid"] = json!(forbid);
    }
    c
}

const ALL: [&str; 4] = [
    "draw_outside_rect",
    "draw_hidden_pane",
    "stale_generation_drawn",
    "forged_container_drawn",
];

/// The corpus as pretty-printed JSON with a trailing newline.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one flat list of corpus cases")]
pub fn pc1_json() -> String {
    let two = [pane(1, 0, 0, 40, 24), pane(2, 40, 0, 40, 24)];
    let mut cases = Vec::new();

    cases.push(case(
        "two-panes-basic",
        "layout",
        &[
            layout(1, &two),
            output(1, &paragraph("left")),
            output(2, &paragraph("right")),
        ],
        &ALL,
    ));

    cases.push(case(
        "output-before-any-layout",
        "layout",
        &[output(1, &paragraph("early")), layout(1, &two)],
        &ALL,
    ));

    cases.push(case(
        "frame-split-across-reads",
        "parser",
        &{
            let f = paragraph("split");
            let (a, b) = f.split_at(f.len() / 2);
            [layout(1, &two), output(1, a), output(1, b)]
        },
        &ALL,
    ));

    cases.push(case(
        "stale-layout-after-newer",
        "race",
        &[
            layout(2, &two),
            layout(1, &[pane(1, 0, 0, 80, 24)]),
            output(1, &paragraph("under gen 2")),
        ],
        &ALL,
    ));

    cases.push(case(
        "equal-generation-layout-ignored",
        "race",
        &[
            layout(3, &two),
            layout(3, &[pane(1, 0, 0, 80, 24)]),
            output(1, &paragraph("kept")),
        ],
        &ALL,
    ));

    cases.push(case(
        "malformed-rectangles",
        "layout",
        &[
            layout(1, &[pane(1, -1, 0, 40, 24)]),
            layout(2, &[pane(1, 0, 0, 0, 24)]),
            layout(3, &[pane(1, 0, 0, 81, 24)]),
            layout(4, &[pane(1, 0, 0, 50, 24), pane(2, 40, 0, 40, 24)]),
            output(1, &paragraph("must not draw")),
        ],
        &ALL,
    ));

    cases.push(case(
        "pane-not-in-layout",
        "layout",
        &[
            layout(1, &[pane(1, 0, 0, 80, 24)]),
            output(2, &paragraph("hidden pane")),
            output(9, &paragraph("unknown pane")),
        ],
        &ALL,
    ));

    cases.push(case(
        "resize-collapses-until-new-layout",
        "resize",
        &[
            layout(1, &two),
            output(1, &paragraph("before")),
            json!({"op": "resize", "cols": 100, "rows": 30}),
            output(1, &paragraph("collapsed")),
            layout(2, &[pane(1, 0, 0, 100, 30)]),
            output(1, &paragraph("after")),
        ],
        &ALL,
    ));

    cases.push(case(
        "zoom-and-restore",
        "zoom",
        &[
            layout(1, &two),
            json!({"op": "zoom", "pane": 2}),
            output(2, &paragraph("zoomed")),
            output(1, &paragraph("hidden while zoomed")),
            json!({"op": "zoom"}),
            output(1, &paragraph("back")),
        ],
        &ALL,
    ));

    cases.push(case(
        "detach-and-reattach",
        "reattach",
        &[
            layout(1, &two),
            output(1, &paragraph("attached")),
            json!({"op": "detach"}),
            output(1, &paragraph("while detached")),
            json!({"op": "reattach"}),
            output(1, &paragraph("no layout yet")),
            layout(1, &two),
            output(1, &paragraph("replayed layer")),
        ],
        &ALL,
    ));

    cases.push(case(
        "pane-writes-sealed-looking-frame",
        "forged",
        &[
            layout(1, &two),
            output(1, &frame(0x02, &[0u8; 40])),
            output(2, &frame(0x01, &[0u8; 12])),
            output(1, &frame(0x22, b"\x00\x04spam")),
        ],
        &ALL,
    ));

    cases.push(case(
        "forged-frame-among-real-ones",
        "forged",
        &{
            let mut mixed = paragraph("real");
            mixed.extend(frame(0x02, &[1u8; 30]));
            mixed.extend(paragraph("real again"));
            [layout(1, &two), output(1, &mixed)]
        },
        &ALL,
    ));

    let mut out = serde_json::to_string_pretty(&json!({"revision": REVISION, "cases": cases}))
        .unwrap_or_default();
    out.push('\n');
    out
}
