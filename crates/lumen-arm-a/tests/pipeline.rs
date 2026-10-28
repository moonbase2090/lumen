//! Arm A at the layer level: rich only on a sealed layer.

use lumen_arm_a::LayerState;
use lumen_arm_a::pipeline::{
    Budget, Config, Context, Counters, Item, Layer, PIN_AFTER_ABORTS, item_summary,
};
use lumen_detector::TAG;
use lumen_sealed::{Direction, DirectionKeys, HEADER_LEN, LayerId, LayerRoot, Receiver, Sender};
use lumen_track0::{Block, KIND_PARAGRAPH};

const LAYER: LayerId = LayerId(7);
const CAP: u8 = 2;

fn cap_of(_kind: u8) -> u8 {
    CAP
}

fn cfg() -> Config {
    Config {
        budget: Budget::quarter_of(400, 40_000),
    }
}

fn keys() -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new([9; 32]), LAYER, Direction::ToTerminal)
}

fn receiver() -> Receiver {
    Receiver::new(keys(), LAYER, Direction::ToTerminal)
}

/// A layer in `state`, given the keys it would have, with a container open.
fn layer(state: LayerState) -> Layer {
    let mut l = Layer::new(cfg(), state);
    l.attach_sealed(receiver(), cap_of);
    l.open_container();
    l
}

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

fn para(text: &str) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(KIND_PARAGRAPH);
    let body_len = u32::try_from(text.len() + 2).unwrap_or(0);
    v.extend_from_slice(&body_len.to_be_bytes());
    v.extend_from_slice(&u16::try_from(text.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(text.as_bytes());
    v
}

/// A detector frame carrying a sealed frame for `plain`.
fn sealed(seq: u64, kind: u8, plain: &[u8]) -> Vec<u8> {
    let mut sender = Sender::new(keys(), LAYER, Direction::ToTerminal, seq);
    let Ok(payload) = sender.seal(kind, CAP, plain) else {
        unreachable!("sealing a short payload succeeds")
    };
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
    v.extend(payload);
    v
}

/// The same frame with one body byte changed: the header verifies and the
/// AEAD check fails.
fn tampered(seq: u64, kind: u8, plain: &[u8]) -> Vec<u8> {
    let mut v = sealed(seq, kind, plain);
    v[2 + TAG.len() + 1 + 4 + HEADER_LEN] ^= 1;
    v
}

fn is_text(items: &[Item]) -> bool {
    matches!(items, [Item::Text(_)])
}

#[test]
fn track0_is_a_block_in_the_container_on_a_sealed_layer() {
    let mut l = layer(LayerState::Sealed);
    assert_eq!(
        l.feed(&para("a"), at(0)),
        vec![Item::Block(Block::Paragraph("a".into()))]
    );
}

#[test]
fn track0_outside_the_container_is_text_on_a_sealed_layer() {
    let mut l = layer(LayerState::Sealed);
    l.close_container();
    assert!(is_text(&l.feed(&para("a"), at(0))));
}

#[test]
fn track0_is_text_on_an_unsealed_layer_even_with_a_container_open() {
    let mut l = layer(LayerState::Unsealed);
    let items = l.feed(&para("a"), at(0));
    assert!(is_text(&items), "{items:?}");
    assert_eq!(l.counters().track0_blocks, 0);
}

#[test]
fn an_unsealed_layer_ignores_keys_so_a_valid_sealed_frame_is_text() {
    let mut l = layer(LayerState::Unsealed);
    let items = l.feed(&sealed(1, 3, b"body"), at(0));
    assert!(is_text(&items), "{items:?}");
    assert_eq!(l.counters().sealed_opened, 0);
}

#[test]
fn a_verified_sealed_frame_opens_and_charges_no_budget() {
    let mut l = Layer::new(
        Config {
            budget: Budget {
                max_frames: 1,
                max_bytes: 1000,
            },
        },
        LayerState::Sealed,
    );
    l.attach_sealed(receiver(), cap_of);
    l.open_container();
    for seq in 1..=20 {
        let items = l.feed(&sealed(seq, 3, b"body"), at(0));
        assert!(
            matches!(items.as_slice(), [Item::Sealed { header, body }] if header.seq == seq && body == b"body"),
            "{items:?}"
        );
    }
    // The unverified budget is untouched, so Track 0 still draws.
    assert!(matches!(
        l.feed(&para("x"), at(0)).as_slice(),
        [Item::Block(_)]
    ));
}

#[test]
fn a_sealed_range_frame_with_a_bad_tag_is_text_and_not_opened() {
    let mut l = layer(LayerState::Sealed);
    let mut bad = sealed(1, 3, b"body");
    bad[2 + TAG.len() + 1 + 4 + 20] ^= 1; // inside the header tag
    let items = l.feed(&bad, at(0));
    assert!(
        !items.iter().any(|i| matches!(i, Item::Sealed { .. })),
        "{items:?}"
    );
    assert_eq!(l.counters().refused_header_invalid, 1);
    assert_eq!(l.counters().sealed_opened, 0);
}

#[test]
fn a_replayed_frame_is_refused_and_counted() {
    let mut l = layer(LayerState::Sealed);
    let frame = sealed(5, 3, b"body");
    assert!(matches!(
        l.feed(&frame, at(0)).as_slice(),
        [Item::Sealed { .. }]
    ));
    let items = l.feed(&frame, at(1));
    assert!(
        !items.iter().any(|i| matches!(i, Item::Sealed { .. })),
        "{items:?}"
    );
    let c = l.counters();
    assert_eq!((c.refused_replay, c.sealed_opened), (1, 1));
}

#[test]
fn bodies_that_fail_the_aead_pin_the_layer_after_the_limit() {
    let mut l = layer(LayerState::Sealed);
    let mut pinned_at = None;
    for n in 0..=PIN_AFTER_ABORTS {
        let seq = u64::try_from(n).unwrap_or(0) + 1;
        let items = l.feed(&tampered(seq, 3, b"body"), at(0));
        assert!(
            items.iter().any(|i| matches!(i, Item::Text(_))),
            "{items:?}"
        );
        if items.contains(&Item::Pinned) {
            pinned_at = Some(n);
        }
    }
    assert_eq!(pinned_at, Some(PIN_AFTER_ABORTS));
    assert!(l.is_pinned());
    let c = l.counters();
    assert_eq!(c.body_aead_failures, 4);
    assert_eq!(c.pinned_notices, 1);
    // Pinned: even a good frame and Track 0 are text now.
    assert!(is_text(&l.feed(&sealed(50, 3, b"ok"), at(1))));
    assert!(is_text(&l.feed(&para("x"), at(1))));
}

#[test]
fn track0_and_header_failures_never_pin() {
    let mut l = layer(LayerState::Sealed);
    for i in 0..50 {
        let mut bad = sealed(1, 3, b"body");
        bad[2 + TAG.len() + 1 + 4 + 20] ^= 1;
        let _ = l.feed(&bad, at(i));
        let _ = l.feed(&para("x"), at(i));
    }
    assert!(!l.is_pinned());
}

#[test]
fn the_unauthenticated_budget_degrades_track0_to_text_once() {
    let mut l = Layer::new(
        Config {
            budget: Budget {
                max_frames: 2,
                max_bytes: 10_000,
            },
        },
        LayerState::Sealed,
    );
    l.open_container();
    assert!(matches!(
        l.feed(&para("a"), at(0)).as_slice(),
        [Item::Block(_)]
    ));
    assert!(matches!(
        l.feed(&para("b"), at(0)).as_slice(),
        [Item::Block(_)]
    ));
    let third = l.feed(&para("c"), at(0));
    assert!(
        matches!(third.as_slice(), [Item::Degraded, Item::Text(_)]),
        "{third:?}"
    );
    assert!(is_text(&l.feed(&para("d"), at(0))));
    assert_eq!(l.counters().degraded_notices, 1);
    // A new command resets the budget.
    l.open_container();
    assert!(matches!(
        l.feed(&para("e"), at(0)).as_slice(),
        [Item::Block(_)]
    ));
}

#[test]
fn alt_screen_and_tmux_or_screen_render_frames_as_text() {
    for ctx in [
        Context {
            alt_screen: true,
            ..Context::default()
        },
        Context {
            tmux_or_screen: true,
            ..Context::default()
        },
    ] {
        let mut l = layer(LayerState::Sealed);
        assert!(is_text(&l.feed(&para("a"), ctx)));
    }
}

#[test]
fn a_frame_split_over_two_reads_is_found() {
    let mut l = layer(LayerState::Sealed);
    let whole = para("split");
    let (a, b) = whole.split_at(5);
    assert_eq!(l.feed(a, at(0)), Vec::<Item>::new());
    assert_eq!(
        l.feed(b, at(0)),
        vec![Item::Block(Block::Paragraph("split".into()))]
    );
}

#[test]
fn counters_add_up_and_the_summary_carries_no_content() {
    let mut l = layer(LayerState::Sealed);
    let _ = l.feed(&sealed(1, 3, b"secret-body"), at(0));
    let _ = l.feed(&para("hello"), at(0));
    let items = l.feed(&sealed(1, 3, b"secret-body"), at(1)); // replay
    let c = l.counters();
    assert_eq!(
        c,
        Counters {
            frames_seen: 3,
            frames_accepted: 2,
            frames_refused: 1,
            refused_replay: 1,
            sealed_opened: 1,
            track0_blocks: 1,
            text_items: items.iter().filter(|i| matches!(i, Item::Text(_))).count() as u64,
            ..Counters::default()
        }
    );
    assert_eq!(
        c.frames_refused,
        c.refused_header_invalid
            + c.refused_replay
            + c.refused_over_header_cap
            + c.refused_over_body_cap
            + c.refused_length_mismatch
            + c.refused_framing
    );
    let summary = item_summary(&Item::Sealed {
        header: lumen_sealed::Header {
            layer: LAYER,
            dir: Direction::ToTerminal,
            origin: 0,
            seq: 1,
            kind: 3,
            len: 11,
        },
        body: b"secret-body".to_vec(),
    });
    assert_eq!(summary, ("sealed", 11));
}
