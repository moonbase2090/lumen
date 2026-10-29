//! The `pc-2` pane corpus (`pc-1` plus two cross-pane cases that Option B can
//! count): scripts for the multiplexer placement comparison.
//!
//! The committed JSON file is the output of [`pc1_json`]; a test fails if the
//! two differ. Pane output is real Track 0 framing, built here with the same
//! framing the arms read, so no hex is typed by hand.

use lumen_detector::TAG;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The corpus revision.
pub const REVISION: &str = "pc-2";

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

fn body(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

/// A pane container the multiplexer sends as given, whatever the layout says.
fn container(pane: u32, generation: u32, rect: [i64; 4], kind: u8, content: &[u8]) -> Value {
    json!({
        "op": "container", "pane": pane, "generation": generation,
        "rect": {"x": rect[0], "y": rect[1], "w": rect[2], "h": rect[3]},
        "kind": kind, "hex": hex(content)
    })
}

const ALL: [&str; 5] = [
    "draw_outside_rect",
    "draw_hidden_pane",
    "stale_generation_drawn",
    "forged_container_drawn",
    "replayed_frame_accepted",
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

    // Cross-pane forgery and replay, written by a pane. Option B has no sealed
    // relay, so these are where it has something to count: every forged frame
    // reaches the outer terminal's pipeline, and every replay writes the same
    // bytes again.
    cases.push(case(
        "cross-pane-forged-container-then-replay",
        "forged",
        &[
            layout(1, &two),
            output(2, &{
                // Pane 2 writes a pane container that claims pane 1's id.
                let mut claim = 1u32.to_be_bytes().to_vec();
                claim.extend_from_slice(&1u32.to_be_bytes());
                claim.extend_from_slice(&[0u8; 32]);
                frame(0x02, &claim)
            }),
            output(1, &paragraph("real")),
            json!({"op": "replay"}),
            json!({"op": "replay"}),
        ],
        &ALL,
    ));
    cases.push(case(
        "forged-layout-from-a-pane-replayed",
        "forged",
        &[
            layout(1, &two),
            output(1, &frame(0x01, &[0u8; 24])),
            json!({"op": "replay"}),
        ],
        &ALL,
    ));

    // A hostile or buggy multiplexer: these are the cases that can fail,
    // because the honest relay above never sends them.
    cases.push(case(
        "hostile-stale-and-early-containers",
        "hostile",
        &[
            layout(2, &two),
            container(1, 1, [0, 0, 40, 24], 0x80, &body("stale")),
            container(1, 3, [0, 0, 40, 24], 0x80, &body("early")),
            container(1, 2, [0, 0, 40, 24], 0x80, &body("current")),
        ],
        &ALL,
    ));

    cases.push(case(
        "hostile-wrong-rectangle",
        "hostile",
        &[
            layout(1, &two),
            container(1, 1, [0, 0, 41, 24], 0x80, &body("too wide")),
            container(
                1,
                1,
                [40, 0, 40, 24],
                0x80,
                &body("another pane's rectangle"),
            ),
            container(1, 1, [70, 0, 20, 24], 0x80, &body("past the edge")),
            container(1, 1, [-5, 0, 40, 24], 0x80, &body("negative origin")),
        ],
        &ALL,
    ));

    cases.push(case(
        "hostile-container-for-unlisted-pane",
        "hostile",
        &[
            layout(1, &[pane(1, 0, 0, 80, 24)]),
            container(2, 1, [0, 0, 80, 24], 0x80, &body("hidden pane")),
            container(99, 1, [0, 0, 80, 24], 0x80, &body("unknown pane")),
        ],
        &ALL,
    ));

    cases.push(case(
        "hostile-container-with-a-sealed-content-type",
        "hostile",
        &[
            layout(1, &two),
            container(1, 1, [0, 0, 40, 24], 0x20, &body("sealed type inside")),
            container(1, 1, [0, 0, 40, 24], 0x80, &[0xff]),
        ],
        &ALL,
    ));

    cases.push(case(
        "hostile-replayed-container",
        "hostile",
        &[
            layout(1, &two),
            container(1, 1, [0, 0, 40, 24], 0x80, &body("once")),
            json!({"op": "replay"}),
            json!({"op": "replay"}),
        ],
        &ALL,
    ));

    cases.push(case(
        "hostile-frame-type-outside-the-keys",
        "hostile",
        &[
            layout(1, &two),
            json!({"op": "foreign", "kind": 16}),
            container(1, 1, [0, 0, 40, 24], 0x80, &body("after desync")),
            output(2, &paragraph("pane text after desync")),
        ],
        &ALL,
    ));

    let mut out = serde_json::to_string_pretty(&json!({"revision": REVISION, "cases": cases}))
        .unwrap_or_default();
    out.push('\n');
    out
}
