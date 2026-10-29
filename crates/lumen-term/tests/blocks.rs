//! Blocks, block-sized PTYs and the arm layer in front of them.

use lumen_detector::TAG;
use lumen_term::{Block, Context, Document, GridBlock, Layer, Notice, Passthrough, Piece};
use lumen_track0::Block as Track0Block;
use std::time::{Duration, Instant};

/// Everything a block's program writes until `needle` shows or time runs out.
fn read_until(block: &mut GridBlock, needle: &str) -> String {
    let mut seen = String::new();
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !seen.contains(needle) {
        let bytes = block.poll_wait(Duration::from_millis(200));
        seen.push_str(&String::from_utf8_lossy(&bytes));
    }
    seen
}

#[cfg(unix)]
#[test]
fn a_blocks_pty_is_the_blocks_size() {
    let mut block = GridBlock::spawn(30, 7, "/bin/sh", &["-c", "stty size"])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(read_until(&mut block, "7 30").contains("7 30"));
}

#[cfg(unix)]
#[test]
fn resizing_a_block_resizes_its_pty() {
    let mut block = GridBlock::spawn(30, 7, "/bin/sh", &["-c", "read x; stty size"])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(block.resize(50, 12).is_ok());
    assert_eq!(block.size(), (50, 12));
    assert!(block.send(b"\n").is_ok());
    assert!(read_until(&mut block, "12 50").contains("12 50"));
}

#[cfg(unix)]
#[test]
fn two_blocks_have_two_ptys_of_their_own_sizes() {
    let mut a = GridBlock::spawn(20, 5, "/bin/sh", &["-c", "stty size"])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let mut b = GridBlock::spawn(64, 3, "/bin/sh", &["-c", "stty size"])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(read_until(&mut a, "5 20").contains("5 20"));
    assert!(read_until(&mut b, "3 64").contains("3 64"));
}

#[cfg(unix)]
#[test]
fn typing_reaches_the_program_and_its_output_reaches_the_grid() {
    let mut block = GridBlock::spawn(40, 5, "/bin/sh", &["-c", "read x; echo got:$x"])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(block.send(b"hi\n").is_ok());
    let seen = read_until(&mut block, "got:hi");
    assert!(seen.contains("got:hi"));
    block.feed(seen.as_bytes());
    assert!(block.text_rows().iter().any(|r| r.contains("got:hi")));
}

#[test]
fn a_block_without_a_program_refuses_input() {
    let mut block = GridBlock::new(10, 2);
    assert!(block.send(b"x").is_err());
    assert_eq!(block.poll(), Vec::<u8>::new());
    assert!(block.has_exited());
}

#[test]
fn passthrough_output_lands_in_one_grid_block() {
    let mut doc = Document::new(20, 4);
    doc.receive(&mut Passthrough, b"hello\r\nworld", at(0));
    let [Block::Grid(g)] = doc.blocks() else {
        unreachable!("one grid block")
    };
    assert_eq!(g.text_rows(), ["hello", "world", "", ""]);
}

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

#[test]
fn pane_grid_osc_clipboard_title_and_notification_sequences_are_not_text() {
    let mut block = GridBlock::new(40, 5);
    block.feed(
        b"before\x1b]52;c;c2VjcmV0\x07\x1b]0;pane title\x07\x1b]9;pane notification\x07after",
    );
    assert_eq!(block.text_rows(), ["beforeafter", "", "", "", ""]);
}

/// A Track 0 paragraph frame.
fn paragraph(text: &str) -> Vec<u8> {
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(0x80);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend(body);
    v
}

fn stream(text: &str) -> Vec<u8> {
    let mut v = b"before\r\n".to_vec();
    v.extend(paragraph(text));
    v.extend_from_slice(b"after\r\n");
    v
}

fn a_layer(sealed: bool) -> lumen_arm_a::pipeline::Layer {
    let state = if sealed {
        lumen_arm_a::LayerState::Sealed
    } else {
        lumen_arm_a::LayerState::Unsealed
    };
    let mut l = lumen_arm_a::pipeline::Layer::new(
        lumen_arm_a::pipeline::Config {
            budget: lumen_arm_a::pipeline::Budget::quarter_of(2000, 8 << 20),
        },
        state,
    );
    Layer::open_container(&mut l);
    l
}

fn b_layer(container: bool) -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    if container {
        Layer::open_container(&mut l);
    }
    l
}

fn kinds(doc: &Document) -> Vec<&'static str> {
    doc.blocks()
        .iter()
        .map(|b| match b {
            Block::Grid(_) => "grid",
            Block::Rich(r) if r.background => "background",
            Block::Rich(_) => "rich",
        })
        .collect()
}

#[test]
fn arm_a_on_a_sealed_layer_makes_a_rich_block_between_two_grid_blocks() {
    let mut doc = Document::new(30, 6);
    let mut layer = a_layer(true);
    doc.receive(&mut layer, &stream("hello"), at(0));
    doc.finish(&mut layer);
    assert_eq!(kinds(&doc), ["grid", "rich", "grid"]);
    let Block::Rich(r) = &doc.blocks()[1] else {
        unreachable!("rich block")
    };
    assert_eq!(r.content, Track0Block::Paragraph("hello".into()));
}

#[test]
fn arm_a_on_an_unsealed_layer_shows_the_same_bytes_as_escaped_text_in_the_grid() {
    let mut doc = Document::new(80, 8);
    let mut layer = a_layer(false);
    doc.receive(&mut layer, &stream("hello"), at(0));
    doc.finish(&mut layer);
    assert_eq!(kinds(&doc), ["grid"]);
}

#[test]
fn arm_b_makes_a_rich_block_from_the_same_bytes_on_any_layer() {
    let mut doc = Document::new(30, 6);
    let mut layer = b_layer(true);
    doc.receive(&mut layer, &stream("hello"), at(0));
    doc.finish(&mut layer);
    assert_eq!(kinds(&doc), ["grid", "rich", "grid"]);
    let Block::Rich(rich) = &doc.blocks()[1] else {
        unreachable!("the Track 0 frame is an inert rich block")
    };
    assert_eq!(rich.content, Track0Block::Paragraph("hello".into()));
    assert!(
        !rich.background,
        "the terminal opened the unverified container"
    );
}

#[test]
fn an_unsealed_program_cannot_turn_a_sealed_power_frame_into_document_state() {
    let mut bytes = b"\x1b_".to_vec();
    bytes.extend_from_slice(&TAG);
    // 0x01 is a sealed command-structure frame type, which has no authority
    // when emitted by an unsealed program.
    bytes.extend_from_slice(&[0x01, 0, 0, 0, 1, b'x']);

    let mut doc = Document::new(30, 6);
    let mut layer = b_layer(true);
    doc.receive(&mut layer, &bytes, at(0));
    doc.finish(&mut layer);

    assert_eq!(kinds(&doc), ["grid"]);
    assert_eq!(doc.sealed(), []);
}

#[test]
fn track_0_with_no_container_is_text_in_arm_a_and_the_background_strip_in_arm_b() {
    let bytes = paragraph("orphan");
    let mut a_doc = Document::new(80, 8);
    let mut a = lumen_arm_a::pipeline::Layer::new(
        lumen_arm_a::pipeline::Config {
            budget: lumen_arm_a::pipeline::Budget::quarter_of(2000, 8 << 20),
        },
        lumen_arm_a::LayerState::Sealed,
    );
    a_doc.receive(&mut a, &bytes, at(0));
    assert_eq!(kinds(&a_doc), ["grid"]);
    let mut b_doc = Document::new(80, 8);
    let mut b = b_layer(false);
    b_doc.receive(&mut b, &bytes, at(0));
    assert_eq!(kinds(&b_doc), ["background"]);
}

#[test]
fn a_notice_from_the_layer_is_kept_and_not_drawn() {
    let mut doc = Document::new(10, 2);
    doc.apply(vec![Piece::Notice(Notice::Degraded)]);
    assert_eq!(doc.notices(), [Notice::Degraded]);
    assert_eq!(doc.blocks().len(), 0);
}

#[test]
fn output_after_a_rich_block_starts_a_new_grid_block() {
    let mut doc = Document::new(30, 4);
    let mut layer = b_layer(true);
    doc.receive(&mut layer, &stream("x"), at(0));
    let grids: Vec<Vec<String>> = doc
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows()),
            Block::Rich(_) => None,
        })
        .collect();
    assert_eq!(grids.len(), 2);
    assert_eq!(grids[0][0], "before");
    assert_eq!(grids[1][0], "after");
}

#[test]
fn too_many_sealed_aborts_pin_the_layer_in_either_arm_and_the_notice_is_kept_once() {
    let mut a_doc = Document::new(10, 2);
    let mut a = a_layer(true);
    let mut b_doc = Document::new(10, 2);
    let mut b = b_layer(true);
    for t in 0..6 {
        a_doc.sealed_abort(&mut a, t);
        b_doc.sealed_abort(&mut b, t);
    }
    assert_eq!(a_doc.notices(), [Notice::Pinned]);
    assert_eq!(b_doc.notices(), [Notice::Pinned]);
}

#[test]
fn a_spent_budget_raises_the_degraded_notice_in_either_arm() {
    let many: Vec<u8> = (0..5).flat_map(|i| paragraph(&format!("p{i}"))).collect();
    let mut a_doc = Document::new(40, 4);
    let mut a = lumen_arm_a::pipeline::Layer::new(
        lumen_arm_a::pipeline::Config {
            budget: lumen_arm_a::pipeline::Budget {
                max_frames: 2,
                max_bytes: 100_000,
            },
        },
        lumen_arm_a::LayerState::Sealed,
    );
    Layer::open_container(&mut a);
    a_doc.receive(&mut a, &many, at(0));
    let mut b_doc = Document::new(40, 4);
    let mut b = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget {
            max_frames: 2,
            max_bytes: 100_000,
        },
        background_as_text: false,
    });
    Layer::open_container(&mut b);
    b_doc.receive(&mut b, &many, at(0));
    assert_eq!(a_doc.notices(), [Notice::Degraded]);
    assert_eq!(b_doc.notices(), [Notice::Degraded]);
}

#[test]
fn escaped_text_takes_its_own_row_and_the_output_after_it_starts_a_new_one() {
    let mut doc = Document::new(80, 6);
    let mut layer = a_layer(false);
    doc.receive(&mut layer, &stream("hi"), at(0));
    let [Block::Grid(g)] = doc.blocks() else {
        unreachable!("one grid block")
    };
    let rows = g.text_rows();
    assert_eq!(rows[0], "before");
    assert!(rows[1].contains("hi"), "{rows:?}");
    assert_eq!(rows[2], "after", "{rows:?}");
}

#[test]
fn the_alternate_screen_and_tmux_reach_the_layer_in_either_arm() {
    for (alt_screen, tmux_or_screen) in [(true, false), (false, true)] {
        let ctx = Context {
            alt_screen,
            tmux_or_screen,
            now_ms: 0,
        };
        let mut a_doc = Document::new(80, 8);
        a_doc.receive(&mut a_layer(true), &stream("x"), ctx);
        assert_eq!(kinds(&a_doc), ["grid"], "arm A, {ctx:?}");
        let mut b_doc = Document::new(80, 8);
        b_doc.receive(&mut b_layer(true), &stream("x"), ctx);
        assert_eq!(kinds(&b_doc), ["grid"], "arm B, {ctx:?}");
    }
}

#[test]
fn blocks_carry_the_prompt_cycle_they_arrived_in() {
    let mut doc = Document::new(30, 6);
    let mut layer = b_layer(true);
    assert_eq!(doc.cycle(), 0);
    doc.receive(&mut layer, &paragraph("one"), at(0));
    doc.begin_prompt_cycle(&mut layer);
    doc.receive(&mut layer, &paragraph("two"), at(1));
    doc.begin_prompt_cycle(&mut layer);
    doc.receive(&mut layer, &paragraph("three"), at(2));
    let cycles: Vec<u32> = doc
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Rich(r) => Some(r.cycle),
            Block::Grid(_) => None,
        })
        .collect();
    assert_eq!(cycles, [0, 1, 2]);
    assert_eq!(doc.cycle(), 2);
}

#[test]
fn a_new_prompt_cycle_resets_the_producers_budget() {
    let many: Vec<u8> = (0..5).flat_map(|i| paragraph(&format!("p{i}"))).collect();
    let mut doc = Document::new(40, 4);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget {
            max_frames: 2,
            max_bytes: 100_000,
        },
        background_as_text: false,
    });
    Layer::open_container(&mut layer);
    doc.receive(&mut layer, &many, at(0));
    assert_eq!(doc.notices(), [Notice::Degraded]);
    doc.begin_prompt_cycle(&mut layer);
    let before = doc.blocks().len();
    doc.receive(&mut layer, &paragraph("fresh"), at(1));
    assert_eq!(
        doc.blocks().len(),
        before + 1,
        "the new cycle has budget again"
    );
}
