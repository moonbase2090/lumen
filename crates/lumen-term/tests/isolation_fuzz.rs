//! The Phase 1 exit test for grid parser isolation: every introducer variant
//! (7-bit, 8-bit C1, UTF-8-encoded C1, string types) with a frame header and a
//! body that has no valid AEAD, at every split position. Nothing is accepted
//! as a frame, and no introducer or control byte reaches the VT parser.

use lumen_detector::TAG;
use lumen_sealed::types::{KIND_NOTIFICATION, capability_of};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver};
use lumen_term::isolate::Isolator;
use lumen_term::{Block, Context, Document, Event, Layer};

const LAYER: LayerId = LayerId(3);

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

/// A layer that holds the key for sealed frames, so only a frame that lacks a
/// valid tag under it can be refused.
fn layer() -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    let keys = DirectionKeys::derive(&LayerRoot::new([4; 32]), LAYER, Direction::ToTerminal);
    l.attach_sealed(Receiver::new(keys, LAYER, Direction::ToTerminal), cap_of);
    Layer::open_container(&mut l);
    l
}

/// Introducers: (name, bytes, whether the 7-bit APC the frame layer owns).
fn introducers() -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<(&'static str, Vec<u8>)> = vec![
        ("apc-7bit", b"\x1b_".to_vec()),
        ("apc-c1", vec![0x9f]),
        ("apc-c1-utf8", vec![0xc2, 0x9f]),
        ("dcs-7bit", b"\x1bP".to_vec()),
        ("dcs-c1", vec![0x90]),
        ("dcs-c1-utf8", vec![0xc2, 0x90]),
        ("pm-7bit", b"\x1b^".to_vec()),
        ("pm-c1", vec![0x9e]),
        ("pm-c1-utf8", vec![0xc2, 0x9e]),
        ("sos-7bit", b"\x1bX".to_vec()),
        ("sos-c1", vec![0x98]),
        ("sos-c1-utf8", vec![0xc2, 0x98]),
        ("osc-52", b"\x1b]52;c;".to_vec()),
        ("osc-c1", vec![0x9d]),
        ("osc-c1-utf8", vec![0xc2, 0x9d]),
        ("csi-c1", vec![0x9b]),
        ("csi-c1-utf8", vec![0xc2, 0x9b]),
        ("esc-then-c1", vec![0x1b, 0x9f]),
        ("esc-then-c1-utf8", vec![0x1b, 0xc2, 0x9f]),
    ];
    v.sort_by_key(|(n, _)| *n);
    v
}

fn terminators() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("none", vec![]),
        ("esc-backslash", b"\x1b\\".to_vec()),
        ("st-c1", vec![0x9c]),
        ("st-c1-utf8", vec![0xc2, 0x9c]),
        ("bel", vec![0x07]),
    ]
}

/// A frame header and a body whose tag was never produced under the key.
fn forged(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = TAG.to_vec();
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// (name, owns the 7-bit APC, whether the stream closes what it opens, bytes)
fn streams() -> Vec<(String, bool, bool, Vec<u8>)> {
    let junk: Vec<u8> = (0..48u8)
        .map(|i| i.wrapping_mul(37).wrapping_add(11))
        .collect();
    let mut out = Vec::new();
    for (iname, intro) in introducers() {
        for kind in [0x01, KIND_NOTIFICATION, 0x80] {
            for (tname, term) in terminators() {
                let mut s = b"before\r\n".to_vec();
                s.extend_from_slice(&intro);
                s.extend(forged(kind, &junk));
                s.extend_from_slice(&term);
                s.extend_from_slice(b"after\r\n");
                // A 7-bit string opened and not closed with ESC \ swallows what
                // follows (and BEL closes only an OSC). That is the rule.
                let closed = match iname {
                    "dcs-7bit" | "pm-7bit" | "sos-7bit" => tname == "esc-backslash",
                    "osc-52" => matches!(tname, "esc-backslash" | "bel"),
                    _ => true,
                };
                out.push((
                    format!("{iname}/{kind:#04x}/{tname}"),
                    iname.contains("apc") || iname.starts_with("esc-then"),
                    closed,
                    s,
                ));
            }
        }
    }
    out
}

/// What reached the document, and what reached the emulator, for one way of
/// cutting the stream into reads.
///
/// The text on the grid is kept for the checks on later output but is not part
/// of the comparison between splits: the escaped text of a rejected header is
/// shown a read at a time, so where a read ends changes where its rows break.
#[derive(Debug)]
struct Outcome {
    kinds: Vec<&'static str>,
    text: String,
    sealed: usize,
    events: usize,
    to_parser: Vec<u8>,
}

impl Outcome {
    fn same_as(&self, other: &Outcome) -> bool {
        self.kinds == other.kinds
            && self.sealed == other.sealed
            && self.events == other.events
            && self.to_parser == other.to_parser
    }
}

fn run(stream: &[u8], cuts: &[usize]) -> Outcome {
    let mut doc = Document::new(60, 8);
    let mut l = layer();
    // The same pipeline again, with the filter's output kept, to see what the
    // emulator was given.
    let mut shadow_layer = layer();
    let mut shadow = Isolator::new();
    let mut to_parser = Vec::new();
    let mut notes = Vec::new();
    let mut start = 0;
    for end in cuts.iter().copied().chain([stream.len()]) {
        let chunk = &stream[start..end];
        start = end;
        doc.receive(&mut l, chunk, Context::default());
        for piece in shadow_layer.feed(chunk, lumen_arm_b::Context::default()) {
            if let lumen_arm_b::Item::Output(bytes) = piece {
                shadow.filter(&bytes, &mut to_parser, &mut notes);
            }
        }
    }
    doc.finish(&mut l);
    for item in shadow_layer.finish() {
        if let lumen_arm_b::Item::Output(bytes) = item {
            shadow.filter(&bytes, &mut to_parser, &mut notes);
        }
    }
    shadow.finish();
    let kinds = doc
        .blocks()
        .iter()
        .map(|b| match b {
            Block::Grid(_) => "grid",
            Block::Rich(r) if r.background => "background",
            Block::Rich(_) => "rich",
        })
        .collect();
    let text = doc
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let events = doc.take_events();
    let sealed = events
        .iter()
        .filter(|e| matches!(e, Event::Sealed { .. }))
        .count()
        + doc.sealed().len();
    Outcome {
        kinds,
        text,
        sealed,
        events: events.len(),
        to_parser,
    }
}

/// Nothing a VT parser could act on as a string, a C1 control or an
/// introducer is in what it was given.
fn assert_inert(name: &str, bytes: &[u8]) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        unreachable!("{name}: the parser was given invalid UTF-8: {bytes:?}")
    };
    assert!(
        !text.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)),
        "{name}: a C1 control reached the parser: {bytes:?}"
    );
    for &intro in b"_P^X" {
        assert!(
            !bytes.windows(2).any(|w| w == [0x1b, intro]),
            "{name}: ESC {} reached the parser",
            char::from(intro)
        );
    }
    assert!(
        !bytes.windows(4).any(|w| w == b"\x1b]52"),
        "{name}: OSC 52 reached the parser"
    );
}

#[test]
fn no_forged_frame_is_accepted_and_nothing_inert_is_not_inert_at_every_split_position() {
    let mut runs = 0;
    for (name, owns_apc, closed, stream) in streams() {
        let whole = run(&stream, &[]);
        assert_eq!(
            whole.sealed, 0,
            "{name}: a frame without a valid tag was accepted"
        );
        assert_eq!(whole.events, 0, "{name}");
        assert_inert(&name, &whole.to_parser);
        if !owns_apc {
            assert!(
                !whole.kinds.contains(&"rich") && !whole.kinds.contains(&"background"),
                "{name}: only the frame layer's own introducer makes a block: {:?}",
                whole.kinds
            );
        }
        assert!(whole.text.contains("before"), "{name}");
        // For the frame layer's own introducers the layer decides how much of a
        // rejected header is shown, so only the rules of the filter are checked
        // on the rest.
        if !owns_apc && closed {
            assert!(
                whole.text.contains("after"),
                "{name}: later output is not swallowed: {:?} {:?}",
                whole.text,
                whole.kinds
            );
        } else if !owns_apc {
            assert!(
                !whole.text.contains("after"),
                "{name}: an unterminated string is discarded to the end"
            );
        }
        for cut in 0..=stream.len() {
            let split = run(&stream, &[cut]);
            assert!(
                split.same_as(&whole),
                "{name}: split at {cut}: {split:?} vs {whole:?}"
            );
            if !owns_apc {
                assert_eq!(
                    split.text.contains("after"),
                    whole.text.contains("after"),
                    "{name}: split at {cut}"
                );
            }
            runs += 1;
        }
        let bytewise: Vec<usize> = (1..stream.len()).collect();
        assert!(
            run(&stream, &bytewise).same_as(&whole),
            "{name}: one byte at a time"
        );
        runs += 1;
    }
    assert!(runs > 20_000, "the matrix is {runs} runs");
}

#[test]
fn a_forged_sealed_frame_is_refused_even_though_the_layer_holds_a_key() {
    // The 7-bit introducer is the frame layer's own, so here the frame is
    // parsed and refused: its tag was not produced under the held key.
    let mut s = b"\x1b_".to_vec();
    s.extend(forged(KIND_NOTIFICATION, &[0xaa; 64]));
    let o = run(&s, &[]);
    assert_eq!((o.sealed, o.events), (0, 0));
    assert_inert("forged", &o.to_parser);
}

#[test]
fn a_dcs_of_8_kib_has_no_effects_on_the_grid_and_the_text_after_it_survives() {
    let mut s = b"one\r\n\x1bP".to_vec();
    s.extend(std::iter::repeat_n(b'x', 8 * 1024));
    s.extend_from_slice(b"\x1b[2J\x1b[Hwiped\x1b\\two\r\n");
    let o = run(&s, &[]);
    assert!(
        o.text.contains("one") && o.text.contains("two"),
        "{:?}",
        o.text
    );
    assert!(!o.text.contains("wiped"), "{:?}", o.text);
    assert!(!o.text.contains('x'));
    let mut doc = Document::new(60, 8);
    doc.receive(&mut lumen_term::Passthrough, &s, Context::default());
    assert!(
        doc.take_events()
            .iter()
            .any(|e| matches!(e, Event::StringTruncated { .. })),
        "the 'string truncated' event is raised"
    );
}
