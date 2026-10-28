//! Both arms must show and charge a rejected header the same way (criteria 2.9).

use lumen_arm_a::LayerState;
use lumen_arm_a::pipeline as a;
use lumen_arm_b as b;
use lumen_detector::TAG;

/// A header whose declared length is over the frame limit, so it is rejected.
fn rejected(intro: &[u8]) -> Vec<u8> {
    let mut v = intro.to_vec();
    v.extend_from_slice(&TAG);
    v.extend_from_slice(&[0x80, 0xff, 0xff, 0xff, 0xff]);
    v
}

/// What an arm produced for a stream: item kinds in order, and the counts that
/// the unauthenticated budget decides.
type Seen = (Vec<&'static str>, [u64; 5]);

fn arm_a(stream: &[u8], max_frames: usize, max_bytes: usize) -> Seen {
    let mut l = a::Layer::new(
        a::Config {
            budget: a::Budget {
                max_frames,
                max_bytes,
            },
        },
        LayerState::Unsealed,
    );
    l.open_container();
    let mut items = l.feed(stream, a::Context::default());
    items.extend(l.finish());
    let c = l.counters();
    (
        items.iter().map(|i| a::item_summary(i).0).collect(),
        [
            c.frames_seen,
            c.frames_refused,
            c.refused_framing,
            c.degraded_notices,
            c.text_items,
        ],
    )
}

fn arm_b(stream: &[u8], max_frames: usize, max_bytes: usize) -> Seen {
    let mut l = b::Layer::new(b::Config {
        budget: b::Budget {
            max_frames,
            max_bytes,
        },
        background_as_text: false,
    });
    l.open_container();
    let mut items = l.feed(stream, b::Context::default());
    items.extend(l.finish());
    let c = l.counters();
    (
        items.iter().map(|i| b::item_summary(i).0).collect(),
        [
            c.frames_seen,
            c.frames_refused,
            c.refused_framing,
            c.degraded_notices,
            c.text_items,
        ],
    )
}

#[test]
fn a_rejected_header_is_shown_once_and_charged_the_same_in_both_arms() {
    for intro in [&b"\x1b_"[..], b"\xc2\x9f", b"\x9f"] {
        for (frames, bytes) in [(1_000, 1_000_000), (2, 1_000_000), (1_000, 30)] {
            let mut stream = Vec::new();
            for _ in 0..6 {
                stream.extend(rejected(intro));
                stream.extend_from_slice(b"after ");
            }
            let seen_a = arm_a(&stream, frames, bytes);
            let seen_b = arm_b(&stream, frames, bytes);
            assert_eq!(seen_a, seen_b, "intro {intro:?} budget {frames}/{bytes}");
            // Each rejected header is text once; nothing shows the introducer
            // as output.
            assert_eq!(seen_a.0.iter().filter(|k| **k == "text").count(), 6);
            assert_eq!(seen_a.1[4], 6);
        }
    }
}

#[test]
fn an_over_budget_rejected_header_shows_once_without_the_introducer() {
    for intro in [&b"\x1b_"[..], b"\xc2\x9f", b"\x9f"] {
        let mut l = a::Layer::new(
            a::Config {
                budget: a::Budget {
                    max_frames: 1,
                    max_bytes: 10,
                },
            },
            LayerState::Unsealed,
        );
        l.open_container();
        let mut items = Vec::new();
        for _ in 0..4 {
            items.extend(l.feed(&rejected(intro), a::Context::default()));
        }
        let texts: Vec<&String> = items
            .iter()
            .filter_map(|i| match i {
                a::Item::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 4, "{items:?}");
        assert!(
            texts.iter().all(|t| !t.contains('\u{241b}')),
            "the introducer must not be shown: {texts:?}"
        );
    }
}
