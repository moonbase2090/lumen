//! The outer terminal's pane handling, driven by the Option A multiplexer.
#![allow(
    clippy::many_single_char_names,
    reason = "short names for the multiplexer, terminal, panes and frames keep the scripts readable"
)]

use lumen_panes::codec::{Layout, Malformed, PaneContainer};
use lumen_panes::{Multiplexer, Outcome, PaneTerminal, Rect, TYPE_LAYOUT, TYPE_PANE};
use lumen_sealed::types::{CAP_NOTIFICATIONS, KIND_CLIPBOARD_WRITE, KIND_NOTIFICATION};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver, Sender};
use lumen_track0::{Block, KIND_PARAGRAPH};

const LAYER: LayerId = LayerId(9);
const COLS: u32 = 80;
const ROWS: u32 = 24;

fn root() -> LayerRoot {
    LayerRoot::new([4; 32])
}

fn pair() -> (Multiplexer, PaneTerminal) {
    (
        Multiplexer::new(&root(), LAYER),
        PaneTerminal::new(&root(), LAYER, COLS, ROWS),
    )
}

fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

fn left() -> Rect {
    r(0, 0, 40, 24)
}

fn right() -> Rect {
    r(40, 0, 40, 24)
}

fn paragraph(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame_with_capability(kind: u8, capability: u8, payload: &[u8]) -> Vec<u8> {
    let root = root();
    let keys = DirectionKeys::derive(&root, LAYER, Direction::ToTerminal);
    let mut sender = Sender::new(keys, LAYER, Direction::ToTerminal, 3);
    let frame = sender.seal(kind, capability, payload);
    assert!(frame.is_ok(), "a short pane frame must seal");
    frame.unwrap_or_default()
}

/// A mux and terminal with a two-pane layout already accepted.
fn two_panes() -> (Multiplexer, PaneTerminal, u32, u32) {
    let (mut m, mut t) = pair();
    let (a, b) = (m.new_pane(), m.new_pane());
    let frame = m.layout(&[(a, left()), (b, right())]);
    assert_eq!(t.receive(&frame), Outcome::LayoutAccepted);
    (m, t, a, b)
}

#[test]
fn a_container_for_a_listed_pane_at_the_current_generation_is_drawn_in_its_rectangle() {
    let (mut m, mut t, a, b) = two_panes();
    let f = m.container(a, left(), KIND_PARAGRAPH, &paragraph("hi"));
    assert_eq!(t.receive(&f), Outcome::Drawn);
    let f = m.container(b, right(), KIND_PARAGRAPH, &paragraph("yo"));
    assert_eq!(t.receive(&f), Outcome::Drawn);
    let d = t.draws();
    assert_eq!(d.len(), 2);
    assert_eq!((d[0].pane, d[0].rect, d[0].generation), (a, left(), 1));
    assert_eq!(d[1].block, Block::Paragraph("yo".into()));
    assert_eq!(t.counters().containers_drawn, 2);
    assert_eq!(t.counters().failed_frames, 0);
}

#[test]
fn no_container_is_drawn_before_a_layout() {
    let (mut m, mut t) = pair();
    let a = m.new_pane();
    let f = m.container_at(1, a, left(), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(t.receive(&f), Outcome::EarlyContainer);
    assert_eq!(t.draws(), []);
    assert_eq!(t.counters().failed_frames, 0);
}

#[test]
fn stale_and_early_containers_are_counted_not_drawn_and_do_not_count_toward_the_pin() {
    let (mut m, mut t, a, _) = two_panes();
    let next = m.layout(&[(a, left())]);
    assert_eq!(t.receive(&next), Outcome::LayoutAccepted);
    let stale = m.container_at(1, a, left(), KIND_PARAGRAPH, &paragraph("old"));
    assert_eq!(t.receive(&stale), Outcome::StaleContainer);
    let early = m.container_at(3, a, left(), KIND_PARAGRAPH, &paragraph("new"));
    assert_eq!(t.receive(&early), Outcome::EarlyContainer);
    assert_eq!(t.draws(), []);
    let c = t.counters();
    assert_eq!((c.containers_stale, c.containers_early), (1, 1));
    assert_eq!(c.failed_frames, 0);
}

#[test]
fn a_layout_must_have_a_greater_generation_than_any_accepted() {
    let (mut m, mut t, a, _) = two_panes();
    let equal = m.layout_at(1, &[(a, left())]);
    assert_eq!(t.receive(&equal), Outcome::StaleLayout);
    let lower = m.layout_at(0, &[(a, left())]);
    assert_eq!(t.receive(&lower), Outcome::StaleLayout);
    assert_eq!(t.visible().len(), 2, "a stale layout has no effect");
    assert_eq!(t.counters().layouts_stale, 2);
    assert_eq!(t.counters().failed_frames, 0);
}

#[test]
fn a_container_for_an_unlisted_pane_or_with_another_rectangle_is_dropped() {
    let (mut m, mut t, a, _) = two_panes();
    let other = m.new_pane();
    let f = m.container(other, left(), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(t.receive(&f), Outcome::DroppedContainer);
    let f = m.container(a, r(0, 0, 41, 24), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(t.receive(&f), Outcome::DroppedContainer);
    assert_eq!(t.draws(), []);
    assert_eq!(t.counters().containers_dropped, 2);
}

#[test]
fn a_new_layout_replaces_the_visible_set() {
    let (mut m, mut t, a, b) = two_panes();
    let f = m.layout(&[(a, r(0, 0, 80, 24))]);
    assert_eq!(t.receive(&f), Outcome::LayoutAccepted);
    assert_eq!(t.visible(), vec![a]);
    let f = m.container(b, right(), KIND_PARAGRAPH, &paragraph("hidden"));
    assert_eq!(t.receive(&f), Outcome::DroppedContainer);
    assert_eq!(t.draws(), []);
}

#[test]
fn a_malformed_layout_is_a_failed_frame_and_changes_nothing() {
    let (mut m, mut t, a, b) = two_panes();
    let cases: Vec<(Vec<(u32, Rect)>, Malformed)> = vec![
        (vec![(a, left()), (a, right())], Malformed::DuplicatePane),
        (vec![(a, r(-1, 0, 10, 10))], Malformed::BadRect),
        (vec![(a, r(0, -1, 10, 10))], Malformed::BadRect),
        (vec![(a, r(0, 0, 0, 10))], Malformed::BadRect),
        (vec![(a, r(0, 0, 10, 0))], Malformed::BadRect),
        (vec![(a, r(71, 0, 10, 10))], Malformed::BadRect),
        (vec![(a, r(0, 15, 10, 10))], Malformed::BadRect),
        (
            vec![(a, r(0, 0, 41, 24)), (b, r(40, 0, 40, 24))],
            Malformed::Overlap,
        ),
    ];
    let before = t.counters().failed_frames;
    for (n, (panes, want)) in cases.iter().enumerate() {
        let f = m.layout(panes);
        assert_eq!(t.receive(&f), Outcome::Malformed(*want), "case {n}");
        assert_eq!(t.visible(), vec![a, b], "case {n} changed the layout");
    }
    assert_eq!(
        t.counters().failed_frames,
        before + u32::try_from(cases.len()).unwrap_or(0)
    );
}

#[test]
fn a_layout_that_ends_exactly_at_the_edge_is_valid_and_one_cell_past_is_not() {
    let (mut m, mut t) = pair();
    let a = m.new_pane();
    let f = m.layout(&[(a, r(0, 0, COLS, ROWS))]);
    assert_eq!(t.receive(&f), Outcome::LayoutAccepted);
    let f = m.layout(&[(a, r(0, 0, COLS + 1, ROWS))]);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::BadRect));
}

#[test]
fn layout_length_and_count_limits_are_checked() {
    let (mut m, mut t) = pair();
    let mut too_many = Layout {
        generation: 5,
        panes: Vec::new(),
    }
    .encode();
    too_many[4..6].copy_from_slice(&257u16.to_be_bytes());
    too_many.extend(vec![0u8; 257 * 20]);
    let f = m.raw(TYPE_LAYOUT, &too_many);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::TooManyPanes));
    let mut short = Layout {
        generation: 6,
        panes: vec![(1, left())],
    }
    .encode();
    short.pop();
    let f = m.raw(TYPE_LAYOUT, &short);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::Length));
    let f = m.raw(TYPE_LAYOUT, &[0, 0]);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::Length));
}

#[test]
fn a_malformed_container_is_a_failed_frame_and_is_not_drawn() {
    let (mut m, mut t, a, _) = two_panes();
    let f = m.container(a, left(), 0x20, &paragraph("sealed kind"));
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::ContentKind));
    let f = m.container(a, r(0, 0, 0, 5), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::BadRect));
    let f = m.raw(TYPE_PANE, &[0u8; 23]);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::Length));
    let mut no_type = PaneContainer {
        pane: a,
        generation: 1,
        rect: left(),
        kind: KIND_PARAGRAPH,
        content: Vec::new(),
    }
    .encode();
    no_type.pop();
    let f = m.raw(TYPE_PANE, &no_type);
    assert_eq!(t.receive(&f), Outcome::Malformed(Malformed::ContentKind));
    assert_eq!(t.draws(), []);
    assert_eq!(t.counters().failed_frames, 4);
}

#[test]
fn content_that_does_not_parse_is_dropped_not_drawn() {
    let (mut m, mut t, a, _) = two_panes();
    let f = m.container(a, left(), KIND_PARAGRAPH, &[0xff]);
    assert_eq!(t.receive(&f), Outcome::UnparsedContent);
    assert_eq!(t.draws(), []);
    assert_eq!(t.counters().containers_unparsed, 1);
}

#[test]
fn a_resize_discards_the_layout_and_needs_a_greater_generation() {
    let (mut m, mut t, a, _) = two_panes();
    t.resize(100, 30);
    assert_eq!(t.visible(), Vec::<u32>::new());
    let f = m.container(a, left(), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(
        t.receive(&f),
        Outcome::EarlyContainer,
        "no layout after a resize"
    );
    let same = m.layout_at(1, &[(a, left())]);
    assert_eq!(t.receive(&same), Outcome::StaleLayout);
    let next = m.layout(&[(a, r(0, 0, 100, 30))]);
    assert_eq!(t.receive(&next), Outcome::LayoutAccepted);
    assert_eq!(t.counters().resets, 1);
}

#[test]
fn a_type_the_keys_do_not_cover_desynchronises_the_layer() {
    let (mut m, mut t, a, _) = two_panes();
    let f = m.raw(0x10, b"command start");
    assert_eq!(t.receive(&f), Outcome::OtherType);
    assert!(t.is_desynchronised());
    assert_eq!(t.visible(), Vec::<u32>::new());
    let f = m.container(a, left(), KIND_PARAGRAPH, &paragraph("x"));
    assert_eq!(t.receive(&f), Outcome::Ignored);
    assert_eq!(t.draws(), []);
    assert_eq!(t.counters().other_type, 1);
    assert_eq!(t.counters().failed_frames, 1);
}

#[test]
fn non_pane_command_and_grant_types_force_text_fallback() {
    for kind in [0x10, 0x11, 0x12, 0x20] {
        let (mut m, mut t, a, _) = two_panes();
        let frame = m.raw(kind, b"not a pane frame");
        assert_eq!(t.receive(&frame), Outcome::OtherType, "type {kind:#04x}");
        assert!(t.is_desynchronised(), "type {kind:#04x}");
        assert_eq!(t.visible(), Vec::<u32>::new(), "type {kind:#04x}");

        let pane = m.container(a, left(), KIND_PARAGRAPH, &paragraph("still text"));
        assert_eq!(t.receive(&pane), Outcome::Ignored, "type {kind:#04x}");
        assert_eq!(t.draws(), [], "type {kind:#04x}");
    }
}

#[test]
fn pane_content_cannot_send_osc52_or_change_the_outer_title() {
    for escape in ["\u{1b}]52;c;secret\u{7}", "\u{1b}]0;pane title\u{7}"] {
        let (mut m, mut t, a, _) = two_panes();
        let frame = m.container(a, left(), KIND_PARAGRAPH, &paragraph(escape));
        assert_eq!(t.receive(&frame), Outcome::UnparsedContent);
        assert_eq!(t.draws(), []);
        assert_eq!(t.counters().containers_unparsed, 1);
    }
}

#[test]
fn pane_clients_cannot_send_notification_or_clipboard_frames() {
    let cases = [
        (KIND_CLIPBOARD_WRITE, 7, b"clipboard".as_slice()),
        (
            KIND_NOTIFICATION,
            CAP_NOTIFICATIONS,
            b"\0\x0cnotification".as_slice(),
        ),
    ];
    for (kind, capability, payload) in cases {
        let (_, mut t, _, _) = two_panes();
        let frame = frame_with_capability(kind, capability, payload);
        assert_eq!(t.receive(&frame), Outcome::Refused, "type {kind:#04x}");
        assert_eq!(t.draws(), [], "type {kind:#04x}");
        assert_eq!(t.counters().refused, 1, "type {kind:#04x}");
    }
}

#[test]
fn multiplexer_and_ordinary_layer_keys_are_separate_purposes() {
    let (mut multiplexer, mut terminal) = pair();
    let pane = multiplexer.new_pane();
    let layout = Layout {
        generation: 1,
        panes: vec![(pane, left())],
    }
    .encode();

    let ordinary_layout = frame_with_capability(TYPE_LAYOUT, 0, &layout);
    assert_eq!(terminal.receive(&ordinary_layout), Outcome::Refused);

    let mux_layout = multiplexer.layout_at(1, &[(pane, left())]);
    let ordinary_keys = DirectionKeys::derive(&root(), LAYER, Direction::ToTerminal);
    let mut ordinary_receiver = Receiver::new(ordinary_keys, LAYER, Direction::ToTerminal);
    assert!(ordinary_receiver.open(0, &mux_layout).is_err());
}

#[test]
fn frames_under_other_keys_tampered_or_replayed_are_refused() {
    let (mut m, mut t, a, _) = two_panes();
    let mut forger = Multiplexer::new(&LayerRoot::new([5; 32]), LAYER);
    let f = forger.container_at(1, a, left(), KIND_PARAGRAPH, &paragraph("forged"));
    assert_eq!(t.receive(&f), Outcome::Refused);
    let mut f = m.container(a, left(), KIND_PARAGRAPH, &paragraph("x"));
    let last = f.len() - 1;
    f[last] ^= 1;
    assert_eq!(t.receive(&f), Outcome::Refused);
    let good = m.container(a, left(), KIND_PARAGRAPH, &paragraph("ok"));
    assert_eq!(t.receive(&good), Outcome::Drawn);
    assert_eq!(t.receive(&good), Outcome::Refused, "a replay is refused");
    assert_eq!(t.draws().len(), 1);
    assert_eq!(t.counters().refused, 3);
    assert_eq!(t.counters().failed_frames, 3);
}

#[test]
fn every_draw_lies_inside_its_panes_rectangle_and_the_area() {
    let (mut m, mut t) = pair();
    let ids: Vec<u32> = (0..4).map(|_| m.new_pane()).collect();
    let panes: Vec<(u32, Rect)> = ids
        .iter()
        .zip([
            r(0, 0, 40, 12),
            r(40, 0, 40, 12),
            r(0, 12, 40, 12),
            r(40, 12, 40, 12),
        ])
        .map(|(i, rect)| (*i, rect))
        .collect();
    assert_eq!(t.receive(&m.layout(&panes)), Outcome::LayoutAccepted);
    for (id, rect) in &panes {
        let f = m.container(*id, *rect, KIND_PARAGRAPH, &paragraph("p"));
        assert_eq!(t.receive(&f), Outcome::Drawn);
    }
    assert_eq!(t.draws().len(), 4);
    for d in t.draws() {
        let Some((_, pane_rect)) = panes.iter().find(|(id, _)| *id == d.pane) else {
            unreachable!("drawn pane is in the layout")
        };
        assert_eq!(d.rect, *pane_rect);
        assert!(d.rect.is_valid(COLS, ROWS));
    }
}

#[test]
fn pane_ids_are_never_reused() {
    let (mut m, _) = pair();
    let ids: Vec<u32> = (0..1000).map(|_| m.new_pane()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len());
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn rect_validity_and_overlap_edges() {
    assert!(r(0, 0, 1, 1).is_valid(1, 1));
    assert!(!r(0, 0, 1, 1).is_valid(0, 1));
    assert!(!r(i32::MAX, 0, u32::MAX, 1).is_valid(u32::MAX, 1));
    assert!(!r(-1, 0, 5, 5).is_valid(80, 24));
    assert!(r(0, 0, 10, 10).overlaps(&r(9, 9, 5, 5)));
    assert!(
        !r(0, 0, 10, 10).overlaps(&r(10, 0, 5, 5)),
        "touching is not overlapping"
    );
    assert!(!r(0, 0, 10, 10).overlaps(&r(0, 10, 5, 5)));
}
