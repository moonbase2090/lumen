//! Arm B Track 0 rules at the layer level.

use lumen_arm_b::{Budget, Config, Context, Item, Layer};
use lumen_detector::TAG;
use lumen_track0::{Block, KIND_PARAGRAPH};

fn cfg() -> Config {
    Config {
        budget: Budget::quarter_of(400, 40_000),
        background_as_text: false,
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

fn no_ctx() -> Context {
    Context::default()
}

#[test]
fn block_goes_to_the_container_only_while_the_terminal_has_one_open() {
    let mut l = Layer::new(cfg());
    assert_eq!(
        l.feed(&para("a"), no_ctx()),
        vec![Item::Background(Block::Paragraph("a".into()))]
    );
    l.open_container();
    assert_eq!(
        l.feed(&para("b"), no_ctx()),
        vec![Item::Block(Block::Paragraph("b".into()))]
    );
    l.close_container();
    assert_eq!(
        l.feed(&para("c"), no_ctx()),
        vec![Item::Background(Block::Paragraph("c".into()))]
    );
}

#[test]
fn background_can_be_forced_to_text() {
    let mut l = Layer::new(Config {
        background_as_text: true,
        ..cfg()
    });
    let items = l.feed(&para("x"), no_ctx());
    assert!(matches!(items.as_slice(), [Item::Text(_)]), "{items:?}");
}

#[test]
fn alt_screen_and_tmux_or_screen_render_frames_as_text() {
    for ctx in [
        Context {
            alt_screen: true,
            tmux_or_screen: false,
        },
        Context {
            alt_screen: false,
            tmux_or_screen: true,
        },
    ] {
        let mut l = Layer::new(cfg());
        l.open_container();
        let items = l.feed(&para("hi"), ctx);
        assert!(
            matches!(items.as_slice(), [Item::Text(t)] if t.contains("hi")),
            "{items:?}"
        );
    }
}

#[test]
fn ordinary_output_passes_through_around_frames() {
    let mut l = Layer::new(cfg());
    l.open_container();
    let mut s = b"before".to_vec();
    s.extend(para("p"));
    s.extend_from_slice(b"after");
    assert_eq!(
        l.feed(&s, no_ctx()),
        vec![
            Item::Output(b"before".to_vec()),
            Item::Block(Block::Paragraph("p".into())),
            Item::Output(b"after".to_vec()),
        ]
    );
}

#[test]
fn bad_sealed_unknown_and_truncated_frames_are_only_text() {
    let mut l = Layer::new(cfg());
    l.open_container();
    // Sealed-range type byte (0x01) is never accepted by the unsealed layer.
    let mut sealed = b"\x1b_".to_vec();
    sealed.extend_from_slice(&TAG);
    sealed.extend_from_slice(&[0x01, 0, 0, 0, 1, b'x']);
    let items = l.feed(&sealed, no_ctx());
    assert!(matches!(items.as_slice(), [Item::Text(_)]), "{items:?}");
    // Malformed payload of a known type.
    let mut bad = para("ok");
    let last = bad.len() - 1;
    bad[last - 3] = 0xff;
    let items = l.feed(&bad, no_ctx());
    assert!(
        items
            .iter()
            .all(|i| !matches!(i, Item::Block(_) | Item::Background(_))),
        "{items:?}"
    );
    // Truncated at end of stream.
    let cut = para("truncate me");
    l.feed(&cut[..cut.len() - 4], no_ctx());
    let items = l.finish();
    assert!(matches!(items.as_slice(), [Item::Text(_)]), "{items:?}");
}

#[test]
fn budget_degrades_the_command_to_text_with_one_notice() {
    let mut l = Layer::new(Config {
        budget: Budget {
            max_frames: 3,
            max_bytes: 10_000,
        },
        background_as_text: false,
    });
    l.open_container();
    let mut notices = 0;
    let mut blocks = 0;
    let mut texts = 0;
    for _ in 0..10 {
        for item in l.feed(&para("x"), no_ctx()) {
            match item {
                Item::Degraded => notices += 1,
                Item::Block(_) => blocks += 1,
                Item::Text(_) => texts += 1,
                _ => {}
            }
        }
    }
    assert_eq!((blocks, notices, texts), (3, 1, 7));
    // A new command gets a fresh budget.
    l.close_container();
    l.open_container();
    assert!(matches!(
        l.feed(&para("y"), no_ctx()).as_slice(),
        [Item::Block(_)]
    ));
}

#[test]
fn byte_budget_also_degrades() {
    let mut l = Layer::new(Config {
        budget: Budget {
            max_frames: 1000,
            max_bytes: 20,
        },
        background_as_text: false,
    });
    l.open_container();
    let items = l.feed(&para(&"z".repeat(100)), no_ctx());
    assert_eq!(items.first(), Some(&Item::Degraded));
}

#[test]
fn rejected_header_is_charged_and_shown_as_text() {
    let mut l = Layer::new(Config {
        budget: Budget {
            max_frames: 2,
            max_bytes: 100_000,
        },
        background_as_text: false,
    });
    l.open_container();
    let mut huge = b"\x1b_".to_vec();
    huge.extend_from_slice(&TAG);
    huge.extend_from_slice(&[KIND_PARAGRAPH, 0xff, 0xff, 0xff, 0xff]);
    let mut texts = 0;
    let mut notices = 0;
    for _ in 0..5 {
        let items = l.feed(&huge, no_ctx());
        texts += items.iter().filter(|i| matches!(i, Item::Text(_))).count();
        notices += items.iter().filter(|i| matches!(i, Item::Degraded)).count();
    }
    assert!(texts >= 5);
    assert_eq!(
        notices, 1,
        "one notice when rejected headers spend the budget"
    );
    // After the budget is spent, a valid block in the same command is text.
    assert!(
        l.feed(&para("late"), no_ctx())
            .iter()
            .all(|i| !matches!(i, Item::Block(_)))
    );
}

#[test]
fn text_items_never_contain_control_characters() {
    let mut l = Layer::new(cfg());
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(&TAG);
    s.extend_from_slice(&[0x80, 0, 0, 0, 5, 0, 3, 0x1b, b']', b'x']);
    for item in l.feed(&s, no_ctx()) {
        if let Item::Text(t) = item {
            assert!(t.chars().all(|c| !c.is_control()), "{t:?}");
        }
    }
}

#[test]
fn truncated_frame_spending_the_budget_gives_the_notice_too() {
    let mut l = Layer::new(Config {
        budget: Budget {
            max_frames: 1000,
            max_bytes: 10,
        },
        background_as_text: false,
    });
    l.open_container();
    let cut = para(&"q".repeat(50));
    let mut items = l.feed(&cut[..cut.len() - 5], no_ctx());
    items.extend(l.finish());
    assert_eq!(
        items.iter().filter(|i| matches!(i, Item::Degraded)).count(),
        1,
        "{items:?}"
    );
}
