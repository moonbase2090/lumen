//! Runs `conformance/vectors/gates.json`: the Track 0 gates G12 (trust margin),
//! G13 (no authority from unsealed input) and G14 (side-channel parity) against
//! the reference terminal with the Arm B layer.

use lumen_detector::TAG;
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver};
use lumen_term::notify::{Desk, Notifier, Place, Recorder, Settings};
use lumen_term::{Context, Document, Event, Layer, Piece, Renderer, Window, layout};
use serde_json::Value;
use std::path::PathBuf;

const LAYER: LayerId = LayerId(3);

fn load() -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../conformance/vectors/gates.json",
    ]
    .iter()
    .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        unreachable!("missing gates.json");
    };
    let Ok(doc) = serde_json::from_str(&text) else {
        unreachable!("gates.json is not valid JSON");
    };
    doc
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| unreachable!("bad hex")))
        .collect()
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// Every input of the file: the named vectors, then each type byte with each payload.
fn inputs(doc: &Value) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for v in doc["vectors"].as_array().map_or(&[][..], Vec::as_slice) {
        let bytes = v["input"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .flat_map(|s| unhex(s["hex"].as_str().unwrap_or_default()))
            .collect();
        out.push((v["id"].as_str().unwrap_or_default().to_string(), bytes));
    }
    for p in doc["frame_kinds"]["payloads"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
    {
        let id = p["id"].as_str().unwrap_or_default();
        let body = unhex(p["hex"].as_str().unwrap_or_default());
        for kind in 0..=255u8 {
            out.push((format!("kind-{kind:#04x}-{id}"), frame(kind, &body)));
        }
    }
    out
}

fn layer(container: bool) -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    // The layer holds a real receiver key, so only "not sealed" stands between
    // the bytes and authority.
    let keys = DirectionKeys::derive(&LayerRoot::new([4; 32]), LAYER, Direction::ToTerminal);
    l.attach_sealed(Receiver::new(keys, LAYER, Direction::ToTerminal), |_| 0);
    if container {
        Layer::open_container(&mut l);
    }
    l
}

/// Feed the bytes whole, or one byte per read.
fn feed(d: &mut Document, l: &mut lumen_arm_b::Layer, bytes: &[u8], single: bool) {
    if single {
        for b in bytes {
            d.receive(l, std::slice::from_ref(b), Context::default());
        }
    } else {
        d.receive(l, bytes, Context::default());
    }
    d.finish(l);
}

fn window(v: &Value) -> Window {
    let n = |k: &str| u32::try_from(v[k].as_u64().unwrap_or(1)).unwrap_or(1);
    Window {
        cols: n("cols"),
        rows: n("rows"),
        editor_row: n("editor_row"),
        editor_rows: n("editor_rows"),
    }
}

fn doc_for(win: &Window) -> Document {
    let cols = usize::try_from(win.cols.saturating_sub(4))
        .unwrap_or(1)
        .max(1);
    let rows = usize::try_from(win.rows.saturating_sub(1))
        .unwrap_or(1)
        .max(1);
    Document::new(cols, rows)
}

#[test]
fn g13_no_unsealed_input_has_authority() {
    let doc = load();
    let mut runs = 0;
    for (name, bytes) in inputs(&doc) {
        for single in [false, true] {
            let mut d = Document::new(60, 8);
            let mut l = layer(true);
            feed(&mut d, &mut l, &bytes, single);
            assert_eq!(d.sealed(), [], "{name}: a sealed frame opened");
            let events = d.take_events();
            assert!(
                events.iter().all(|e| !matches!(e, Event::Sealed { .. })),
                "{name}: sealed event"
            );
            let mut n = Notifier::new(Settings::default());
            n.set_desk(Desk {
                focused: false,
                muted: false,
            });
            assert!(n.add_layer(1, "tab 1", Place::Local));
            let mut sink = Recorder::default();
            n.route(1000, 1, &events, &mut sink);
            assert!(!n.granted(1), "{name}: a grant was made");
            assert_eq!(n.shown(), [], "{name}: a toast was shown");
            assert_eq!(sink.sent, [], "{name}: the OS was notified");
            runs += 1;
        }
    }
    assert!(runs > 1800, "too few runs: {runs}");
}

#[test]
fn g14_frames_never_reach_the_parser_and_never_vanish() {
    let doc = load();
    for (name, bytes) in inputs(&doc) {
        if !bytes.starts_with(b"\x1b_") {
            continue;
        }
        for single in [false, true] {
            let mut l = layer(true);
            let mut pieces = Vec::new();
            if single {
                for b in &bytes {
                    pieces.extend(Layer::feed(
                        &mut l,
                        std::slice::from_ref(b),
                        Context::default(),
                    ));
                }
            } else {
                pieces.extend(Layer::feed(&mut l, &bytes, Context::default()));
            }
            pieces.extend(Layer::finish(&mut l));
            for p in &pieces {
                match p {
                    Piece::Bytes(b) => assert!(
                        !b.windows(2).any(|w| w == b"\x1b_") && !b.contains(&0x9f),
                        "{name}: frame bytes reached the parser"
                    ),
                    Piece::Text(t) => assert!(
                        !t.chars().any(char::is_control),
                        "{name}: control character in visible text"
                    ),
                    Piece::Sealed { .. } => unreachable!("{name}: opened as sealed"),
                    Piece::Rich(_) | Piece::Background(_) | Piece::Notice(_) => {}
                }
            }
            assert!(!pieces.is_empty(), "{name}: a frame produced nothing");
        }
    }
}

#[test]
fn g12_no_input_changes_a_margin_or_editor_pixel() {
    let doc = load();
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"));
    let (cw, ch) = r.cell();
    for w in doc["windows"].as_array().map_or(&[][..], Vec::as_slice) {
        let win = window(w);
        let base = r.render_window(&doc_for(&win), &win);
        let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
        let parts = [lay.margin[0], lay.margin[1], lay.editor];
        // Pixels are costly to compare: every type byte is run with the text
        // payload only, the named inputs in full.
        for (name, bytes) in inputs(&doc)
            .into_iter()
            .filter(|(n, _)| !n.starts_with("kind-") || n.ends_with("-text"))
        {
            for container in [true, false] {
                let mut d = doc_for(&win);
                let mut l = layer(container);
                feed(&mut d, &mut l, &bytes, false);
                let f = r.render_window(&d, &win);
                for rect in &parts {
                    for y in rect.y * ch..(rect.y + rect.h) * ch {
                        for x in rect.x * cw..(rect.x + rect.w) * cw {
                            assert_eq!(
                                base.pixel(x, y),
                                f.pixel(x, y),
                                "{name}: {}x{} container {container}: pixel ({x},{y}) changed",
                                win.cols,
                                win.rows
                            );
                        }
                    }
                }
            }
        }
    }
}
