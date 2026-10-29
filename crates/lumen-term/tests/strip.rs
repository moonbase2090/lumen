//! The background output strip and where it sits.

use lumen_detector::TAG;
use lumen_term::layout::{GUTTER_COLUMNS, HEADER_ROWS};
use lumen_term::{Context, Document, Layer, Notice, Rect, StripPlacement, layout};

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

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

fn b_layer(budget_frames: usize, as_text: bool) -> lumen_arm_b::Layer {
    lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget {
            max_frames: budget_frames,
            max_bytes: 1 << 20,
        },
        background_as_text: as_text,
    })
}

fn a_layer() -> lumen_arm_a::pipeline::Layer {
    lumen_arm_a::pipeline::Layer::new(
        lumen_arm_a::pipeline::Config {
            budget: lumen_arm_a::pipeline::Budget::quarter_of(2000, 8 << 20),
        },
        lumen_arm_a::LayerState::Sealed,
    )
}

#[test]
fn background_output_is_one_collapsed_strip_per_cycle_with_a_count_label() {
    let mut doc = Document::new(40, 4);
    let mut layer = b_layer(100, false);
    for t in ["a", "b", "c"] {
        doc.receive(&mut layer, &paragraph(t), at(0));
    }
    doc.begin_prompt_cycle(&mut layer);
    Layer::close_container(&mut layer);
    doc.receive(&mut layer, &paragraph("d"), at(1));
    let strips = doc.strips();
    assert_eq!(strips.len(), 2, "one strip for each cycle with output");
    assert_eq!((strips[0].cycle, strips[0].count()), (0, 3));
    assert_eq!((strips[1].cycle, strips[1].count()), (1, 1));
    assert_eq!(strips[0].label(), "background output (3)");
    assert!(strips.iter().all(|s| !s.open), "collapsed by default");
}

#[test]
fn the_label_holds_only_a_count() {
    let mut doc = Document::new(40, 4);
    let mut layer = b_layer(100, false);
    doc.receive(&mut layer, &paragraph("secret words"), at(0));
    let label = doc.strips()[0].label();
    assert!(
        !label.contains("secret") && !label.contains("words"),
        "{label}"
    );
}

#[test]
fn only_a_user_action_opens_a_strip() {
    let mut doc = Document::new(40, 4);
    let mut layer = b_layer(100, false);
    // Output, a new cycle, a notice, more output: none of it opens a strip.
    doc.receive(&mut layer, &paragraph("a"), at(0));
    doc.begin_prompt_cycle(&mut layer);
    Layer::close_container(&mut layer);
    doc.receive(&mut layer, &paragraph("b"), at(1));
    doc.finish(&mut layer);
    assert!(doc.strips().iter().all(|s| !s.open));
    assert!(!doc.user_opens_strip(7), "no strip in that cycle");
    assert!(doc.user_opens_strip(0));
    let strips = doc.strips();
    assert!(strips[0].open && !strips[1].open);
    // Output arriving later does not change what the user chose.
    doc.receive(&mut layer, &paragraph("c"), at(2));
    assert!(doc.strips()[0].open && !doc.strips()[1].open);
    doc.user_closes_strip(0);
    assert!(doc.strips().iter().all(|s| !s.open));
}

#[test]
fn arm_a_has_no_strip_and_shows_orphan_output_as_text() {
    let mut doc = Document::new(40, 6);
    let mut layer = a_layer();
    doc.receive(&mut layer, &paragraph("orphan"), at(0));
    assert_eq!(doc.strips().len(), 0);
}

#[test]
fn a_spent_budget_stops_the_strip_growing_and_raises_the_notice() {
    let mut doc = Document::new(40, 6);
    let mut layer = b_layer(2, false);
    let many: Vec<u8> = (0..6).flat_map(|i| paragraph(&format!("p{i}"))).collect();
    doc.receive(&mut layer, &many, at(0));
    assert_eq!(doc.strips()[0].count(), 2, "no more than the budget");
    assert_eq!(doc.notices(), [Notice::Degraded]);
}

#[test]
fn background_track0_text_sends_orphan_output_to_the_grid_as_escaped_text() {
    let mut doc = Document::new(60, 6);
    let mut layer = b_layer(100, true);
    doc.receive(&mut layer, &paragraph("orphan"), at(0));
    assert_eq!(doc.strips().len(), 0, "the policy leaves no strip");
    let text = doc
        .blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
            lumen_term::Block::Rich(_) => None,
        })
        .collect::<String>();
    assert!(text.contains("orphan"), "{text:?}");
}

#[test]
fn track_0_in_an_open_container_is_not_background() {
    let mut doc = Document::new(40, 6);
    let mut layer = b_layer(100, false);
    Layer::open_container(&mut layer);
    doc.receive(&mut layer, &paragraph("in the container"), at(0));
    assert_eq!(doc.strips().len(), 0);
}

#[test]
fn the_alternate_screen_and_multiplexer_rules_leave_no_strip() {
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
        let mut doc = Document::new(40, 6);
        let mut layer = b_layer(100, false);
        doc.receive(&mut layer, &paragraph("x"), ctx);
        assert_eq!(doc.strips().len(), 0, "{ctx:?}");
    }
}

// ---- layout ----

fn rects_in(l: &lumen_term::Layout) -> Vec<Rect> {
    let mut v = l.margin.to_vec();
    v.push(l.content);
    v.push(l.editor);
    if let StripPlacement::Placed(r) = l.strip {
        v.push(r);
    }
    v
}

#[test]
fn the_margin_is_the_gutter_and_the_header_strip() {
    let l = layout(80, 24, 20, 1, None);
    assert_eq!(
        l.margin[0],
        Rect {
            x: 0,
            y: 0,
            w: GUTTER_COLUMNS,
            h: 24
        }
    );
    assert_eq!(
        l.margin[1],
        Rect {
            x: 0,
            y: 0,
            w: 80,
            h: HEADER_ROWS
        }
    );
    assert!(!l.margin[0].overlaps(&l.content) && !l.margin[1].overlaps(&l.content));
    assert!(!l.margin[0].overlaps(&l.editor) && !l.margin[1].overlaps(&l.editor));
}

#[test]
fn the_strip_sits_below_the_editor_inside_the_content_and_off_the_margin() {
    for open in [false, true] {
        let l = layout(80, 24, 18, 2, Some((open, 3)));
        let StripPlacement::Placed(s) = l.strip else {
            unreachable!("there is room")
        };
        assert_eq!(s.y, l.editor.bottom(), "directly below the editor");
        assert!(l.content.contains(&s));
        assert!(!s.overlaps(&l.editor));
        assert!(l.margin.iter().all(|m| !m.overlaps(&s)));
        assert_eq!(
            s.h,
            if open { 4 } else { 1 },
            "header plus a row per block when open"
        );
    }
}

#[test]
fn an_open_strip_is_clipped_to_the_window_and_never_moves_the_editor() {
    let l = layout(80, 24, 20, 1, Some((true, 50)));
    let StripPlacement::Placed(s) = l.strip else {
        unreachable!("there is room")
    };
    assert_eq!(s.bottom(), 24, "clipped at the bottom of the window");
    assert_eq!(l.editor, layout(80, 24, 20, 1, None).editor);
}

#[test]
fn with_no_row_below_the_editor_the_strip_has_no_room_and_nothing_moves() {
    let none = layout(80, 24, 23, 1, None);
    let full = layout(80, 24, 23, 1, Some((true, 5)));
    assert_eq!(full.strip, StripPlacement::NoRoom);
    assert_eq!(
        (full.editor, full.margin, full.content),
        (none.editor, none.margin, none.content)
    );
}

/// A small deterministic generator, so the property test needs no dependency.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

#[test]
fn no_sequence_of_output_and_user_actions_moves_the_prompt_or_the_margin() {
    let (cols, rows) = (80u32, 24u32);
    for seed in 0..40u64 {
        let mut rng = Lcg(seed + 1);
        let editor_row = u32::try_from(rng.next(u64::from(rows - 1))).unwrap_or(0);
        let editor_rows = 1 + u32::try_from(rng.next(3)).unwrap_or(0);
        let base = layout(cols, rows, editor_row, editor_rows, None);
        let mut doc = Document::new(40, 4);
        let mut layer = b_layer(1 + usize::try_from(rng.next(20)).unwrap_or(0), false);
        for step in 0..60 {
            match rng.next(6) {
                0 | 1 => doc.receive(&mut layer, &paragraph(&format!("o{step}")), at(0)),
                2 => {
                    doc.begin_prompt_cycle(&mut layer);
                    Layer::close_container(&mut layer);
                }
                3 => {
                    let cycle = u32::try_from(rng.next(5)).unwrap_or(0);
                    doc.user_opens_strip(cycle);
                }
                4 => doc.user_closes_strip(u32::try_from(rng.next(5)).unwrap_or(0)),
                _ => doc.receive(&mut layer, b"plain output\r\n", at(0)),
            }
            // The layout for the newest strip, as the terminal would ask for it.
            let strips = doc.strips();
            let last = strips.last().map(|s| (s.open, s.count()));
            let l = layout(cols, rows, editor_row, editor_rows, last);
            assert_eq!(
                l.editor, base.editor,
                "seed {seed} step {step}: editor moved"
            );
            assert_eq!(
                l.margin, base.margin,
                "seed {seed} step {step}: margin changed"
            );
            assert_eq!(l.content, base.content);
            let all = rects_in(&l);
            for (i, a) in all.iter().enumerate() {
                for (j, b) in all.iter().enumerate().skip(i + 1) {
                    // The two margin rectangles meet at the corner by design,
                    // and the content rectangle contains the editor and the
                    // strip. Everything else must be disjoint.
                    let margins = i < 2 && j < 2;
                    let content = *a == l.content || *b == l.content;
                    if !margins && !content {
                        assert!(!a.overlaps(b), "seed {seed} step {step}: {a:?} {b:?}");
                    }
                }
            }
        }
    }
}
