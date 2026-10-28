//! Counters and item summaries: exact, and consistent with each other.

use lumen_arm_b::{Budget, Config, Context, Counters, Item, Layer, item_summary};
use lumen_detector::TAG;
use lumen_sealed::{Direction, DirectionKeys, HEADER_LEN, LayerId, LayerRoot, Receiver, Sender};
use lumen_track0::{Block, KIND_PARAGRAPH};

const LAYER: LayerId = LayerId(7);
const CAP: u8 = 2;

fn keys() -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new([9; 32]), LAYER, Direction::ToTerminal)
}

fn cap_of(_kind: u8) -> u8 {
    CAP
}

fn layer() -> Layer {
    let mut l = Layer::new(Config {
        budget: Budget::quarter_of(400_000, 40_000_000),
        background_as_text: false,
    });
    l.attach_sealed(Receiver::new(keys(), LAYER, Direction::ToTerminal), cap_of);
    l.open_container();
    l
}

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

fn wrap(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn sealed(seq: u64, plain: &[u8]) -> Vec<u8> {
    let Ok(payload) = Sender::new(keys(), LAYER, Direction::ToTerminal, seq).seal(3, CAP, plain)
    else {
        unreachable!("sealing a short payload succeeds")
    };
    wrap(3, &payload)
}

fn para(text: &str) -> Vec<u8> {
    let mut payload = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    payload.extend_from_slice(text.as_bytes());
    wrap(KIND_PARAGRAPH, &payload)
}

/// The invariants that must hold for any run.
fn consistent(c: &Counters) {
    assert_eq!(c.frames_seen, c.frames_accepted + c.frames_refused, "{c:?}");
    let causes = c.refused_header_invalid
        + c.refused_replay
        + c.refused_over_header_cap
        + c.refused_over_body_cap
        + c.refused_length_mismatch
        + c.refused_framing;
    assert_eq!(c.frames_refused, causes, "{c:?}");
}

#[test]
fn a_fresh_layer_has_all_zero_counters() {
    assert_eq!(layer().counters(), Counters::default());
}

#[test]
fn track_0_blocks_and_background_are_counted() {
    let mut l = layer();
    for _ in 0..3 {
        let _ = l.feed(&para("a"), at(0));
    }
    l.close_container();
    let _ = l.feed(&para("b"), at(0));
    let c = l.counters();
    assert_eq!((c.track0_blocks, c.background_blocks), (3, 1));
    assert_eq!(
        (c.frames_seen, c.frames_accepted, c.frames_refused),
        (4, 4, 0)
    );
    consistent(&c);
}

#[test]
fn each_refusal_cause_is_counted_once() {
    let mut l = layer();
    let _ = l.feed(&sealed(5, b"ok"), at(0));
    // Replay of sequence 5.
    let _ = l.feed(&sealed(5, b"ok"), at(1));
    // Broken header tag.
    let mut forged = sealed(6, b"ok");
    forged[2 + TAG.len() + 1 + 4 + 20] ^= 1;
    let _ = l.feed(&forged, at(2));
    // Authentic header, detector carries fewer bytes than the header declares.
    let mut short = sealed(7, b"hello world");
    let new_len = u32::try_from(HEADER_LEN + 4).unwrap_or(0).to_be_bytes();
    short[2 + TAG.len() + 1..2 + TAG.len() + 5].copy_from_slice(&new_len);
    short.truncate(2 + TAG.len() + 5 + HEADER_LEN + 4);
    let _ = l.feed(&short, at(3));
    // A header over the detector's length limit.
    let mut huge = b"\x1b_".to_vec();
    huge.extend_from_slice(&TAG);
    huge.extend_from_slice(&[3, 0xff, 0xff, 0xff, 0xff]);
    let _ = l.feed(&huge, at(4));
    // Authentic header, body fails the AEAD check.
    let mut bad = sealed(8, b"secret");
    let at_body = 2 + TAG.len() + 1 + 4 + HEADER_LEN;
    bad[at_body] ^= 1;
    let _ = l.feed(&bad, at(5));
    let c = l.counters();
    assert_eq!(c.sealed_opened, 1);
    assert_eq!(c.refused_replay, 1, "{c:?}");
    assert_eq!(c.refused_header_invalid, 1, "{c:?}");
    assert_eq!(c.refused_length_mismatch, 1, "{c:?}");
    assert_eq!(c.refused_framing, 1, "{c:?}");
    assert_eq!(c.body_aead_failures, 1, "{c:?}");
    assert_eq!(c.frames_refused, 4);
    assert_eq!(c.frames_accepted, 2, "the valid frame and the tampered one");
    consistent(&c);
}

#[test]
fn caps_are_counted_by_cause() {
    let mut l = layer();
    for seq in 1..=300u64 {
        let _ = l.feed(&sealed(seq, b"a"), at(5));
    }
    let c = l.counters();
    assert_eq!(
        (c.sealed_opened, c.refused_over_body_cap),
        (256, 44),
        "{c:?}"
    );
    consistent(&c);

    let mut l = layer();
    let mut bad = sealed(1, b"a");
    bad[2 + TAG.len() + 1 + 4 + 20] ^= 1;
    for _ in 0..4100 {
        let _ = l.feed(&bad, at(10));
    }
    let c = l.counters();
    assert_eq!(c.refused_header_invalid, 4096, "{c:?}");
    assert_eq!(c.refused_over_header_cap, 4, "{c:?}");
    consistent(&c);
}

#[test]
fn notices_pins_and_truncation_are_counted() {
    let mut l = layer();
    // Four body failures within a minute pin the layer.
    for i in 0..4u64 {
        let mut bad = sealed(10 + i, b"x");
        let at_body = 2 + TAG.len() + 1 + 4 + HEADER_LEN;
        bad[at_body] ^= 1;
        let _ = l.feed(&bad, at(i));
    }
    let c = l.counters();
    assert_eq!((c.body_aead_failures, c.pinned_notices), (4, 1), "{c:?}");
    // A stream that ends inside a frame.
    let mut l = layer();
    let p = para("truncate me");
    let _ = l.feed(&p[..p.len() - 3], at(0));
    let _ = l.finish();
    assert_eq!(l.counters().truncated, 1);
    // The budget runs out: one notice.
    let mut l = Layer::new(Config {
        budget: Budget {
            max_frames: 2,
            max_bytes: 10_000,
        },
        background_as_text: false,
    });
    l.open_container();
    for _ in 0..6 {
        let _ = l.feed(&para("x"), at(0));
    }
    let c = l.counters();
    assert_eq!(
        (c.degraded_notices, c.track0_blocks, c.text_items),
        (1, 2, 4),
        "{c:?}"
    );
}

#[test]
fn item_summaries_carry_lengths_and_no_content() {
    assert_eq!(item_summary(&Item::Output(vec![1, 2, 3])), ("output", 3));
    assert_eq!(
        item_summary(&Item::Block(Block::Paragraph("hello".into()))),
        ("block", 5)
    );
    assert_eq!(
        item_summary(&Item::Background(Block::Code {
            lang: "rs".into(),
            text: "fn".into(),
            stripped: 0
        })),
        ("background", 4)
    );
    assert_eq!(item_summary(&Item::Text("\u{241b}ab".into())), ("text", 3));
    assert_eq!(item_summary(&Item::Degraded), ("degraded", 0));
    assert_eq!(item_summary(&Item::Pinned), ("pinned", 0));
}

#[test]
fn item_kinds_lists_every_kind_item_summary_returns() {
    use lumen_arm_b::{ITEM_KINDS, Item, item_summary};
    use lumen_track0::Block;
    let block = || Block::Paragraph("x".into());
    let items = [
        Item::Output(vec![1]),
        Item::Block(block()),
        Item::Background(block()),
        Item::Text("x".into()),
        Item::Degraded,
        Item::Pinned,
    ];
    let mut seen: Vec<&str> = items.iter().map(|i| item_summary(i).0).collect();
    seen.push("sealed");
    seen.sort_unstable();
    let mut all = ITEM_KINDS.to_vec();
    all.sort_unstable();
    assert_eq!(seen, all);
}
