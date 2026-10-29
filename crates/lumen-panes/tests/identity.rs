//! How panes are named in trust UI.

use lumen_panes::identity::{
    CLOSE_HUE_DEG, GLYPHS, MARKER_MS, REUSE_WINDOW_MS, colour_of_hue, hue_distance, hue_of,
    reading_order,
};
use lumen_panes::{Identity, PaneLabel, Rect};

fn rect(x: i32, y: i32) -> Rect {
    Rect { x, y, w: 10, h: 5 }
}

fn layout(table: &mut Identity, window: u32, panes: &[(u32, u32)], at: u64) {
    table.apply_layout(window, panes, at);
}

#[test]
fn a_pane_id_gives_the_same_colour_every_time_and_different_ids_differ() {
    assert_eq!(hue_of(7), hue_of(7));
    assert_ne!(hue_of(1), hue_of(2));
    assert_eq!(
        PaneLabel::new(1, 1, 7).colour,
        PaneLabel::new(9, 4, 7).colour
    );
}

#[test]
fn the_colour_derivation_is_pinned() {
    for (id, hue, colour) in [
        (1, 306, [189, 40, 174]),
        (2, 247, [58, 40, 189]),
        (3, 351, [189, 40, 63]),
        (8, 136, [40, 189, 80]),
    ] {
        assert_eq!(hue_of(id), hue, "hue of {id}");
        assert_eq!(colour_of_hue(hue), colour, "colour of {id}");
    }
    assert_eq!(colour_of_hue(0), [189, 40, 40]);
    assert_eq!(colour_of_hue(60), [189, 189, 40]);
    assert_eq!(colour_of_hue(120), [40, 189, 40]);
    assert_eq!(colour_of_hue(240), [40, 40, 189]);
    assert_eq!(colour_of_hue(360), colour_of_hue(0), "the wheel wraps");
}

#[test]
fn every_hue_is_a_valid_colour_at_the_fixed_saturation_and_lightness() {
    for hue in 0..360 {
        let [r, g, b] = colour_of_hue(hue);
        assert_eq!(r.max(g).max(b), 189, "hue {hue}");
        assert_eq!(r.min(g).min(b), 40, "hue {hue}");
    }
}

#[test]
fn hue_distance_goes_the_short_way_round() {
    assert_eq!(hue_distance(10, 350), 20);
    assert_eq!(hue_distance(350, 10), 20);
    assert_eq!(hue_distance(0, 180), 180);
    assert_eq!(hue_distance(90, 90), 0);
}

#[test]
fn the_label_text_is_position_and_glyph_and_nothing_a_program_wrote() {
    let mut l = PaneLabel::new(2, 3, 9);
    assert_eq!(l.text(), "window 2, pane 3");
    l.glyph = Some(GLYPHS[0]);
    assert_eq!(l.text(), format!("window 2, pane 3 {}", GLYPHS[0]));
}

#[test]
fn panes_are_indexed_in_reading_order() {
    let panes = [(9, rect(10, 5)), (4, rect(0, 5)), (7, rect(0, 0))];
    assert_eq!(reading_order(&panes), [(7, 1), (4, 2), (9, 3)]);
    assert_eq!(
        reading_order(&[(1, rect(0, 5)), (2, rect(10, 0))]),
        [(2, 1), (1, 2)],
        "a pane on a higher row comes first, whatever its column"
    );
    assert_eq!(
        reading_order(&[(2, rect(0, 0)), (1, rect(0, 0))]),
        [(1, 1), (2, 2)],
        "equal positions break by id"
    );
}

#[test]
fn a_pane_with_a_distant_colour_has_no_glyph() {
    let mut t = Identity::new();
    // Hues 306 and 136 are far apart.
    layout(&mut t, 1, &[(1, 1), (8, 2)], 0);
    let labels = t.labels(1);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l.glyph.is_none()));
}

#[test]
fn panes_with_close_colours_each_get_a_different_glyph() {
    let mut t = Identity::new();
    // Hues 247 and 250 are three degrees apart.
    layout(&mut t, 1, &[(2, 1), (6, 2), (1, 3)], 0);
    let labels = t.labels(1);
    let (a, b) = (t.label(1, 2), t.label(1, 6));
    let (a, b) = (a.map(|l| l.glyph), b.map(|l| l.glyph));
    assert!(a.is_some() && b.is_some());
    assert_ne!(a, b, "the two close panes tell apart");
    assert_eq!(
        t.label(1, 1).map(|l| l.glyph),
        Some(None),
        "the distant pane has none"
    );
    assert_eq!(labels.len(), 3);
}

#[test]
fn closeness_has_a_threshold_at_the_documented_angle() {
    let near = hue_of(2);
    let ids: Vec<u32> = (1..400).collect();
    // Find a pair exactly at the threshold and one just past it.
    let at = ids
        .iter()
        .find(|id| hue_distance(near, hue_of(**id)) == CLOSE_HUE_DEG && **id != 2);
    let past = ids
        .iter()
        .find(|id| hue_distance(near, hue_of(**id)) == CLOSE_HUE_DEG + 1);
    let (Some(at), Some(past)) = (at, past) else {
        unreachable!("ids 1..400 cover both distances")
    };
    let mut t = Identity::new();
    layout(&mut t, 1, &[(2, 1), (*at, 2)], 0);
    assert!(
        t.label(1, 2).and_then(|l| l.glyph).is_some(),
        "at the threshold is close"
    );
    let mut t = Identity::new();
    layout(&mut t, 1, &[(2, 1), (*past, 2)], 0);
    assert_eq!(t.label(1, 2).and_then(|l| l.glyph), None, "past it is not");
}

#[test]
fn glyphs_do_not_cross_windows() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(2, 1)], 0);
    layout(&mut t, 2, &[(6, 1)], 0);
    assert_eq!(t.label(1, 2).and_then(|l| l.glyph), None);
    assert_eq!(t.label(2, 6).and_then(|l| l.glyph), None);
}

#[test]
fn a_position_taken_by_another_pane_soon_after_shows_the_changed_marker() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(1, 1), (2, 2)], 1_000);
    assert!(!t.changed_marker(1, 2, 1_000));
    // Pane 2 leaves; pane 3 takes position 2 within the window.
    layout(&mut t, 1, &[(1, 1)], 5_000);
    layout(&mut t, 1, &[(1, 1), (3, 2)], 5_000 + REUSE_WINDOW_MS - 1);
    let at = 5_000 + REUSE_WINDOW_MS - 1;
    assert!(t.changed_marker(1, 2, at));
    assert!(t.changed_marker(1, 2, at + MARKER_MS - 1));
    assert!(!t.changed_marker(1, 2, at + MARKER_MS), "the marker lapses");
    assert!(!t.changed_marker(1, 1, at), "pane 1 never changed");
}

#[test]
fn a_position_taken_after_the_window_has_no_marker() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(1, 1), (2, 2)], 0);
    layout(&mut t, 1, &[(1, 1)], 1_000);
    layout(&mut t, 1, &[(1, 1), (3, 2)], 1_000 + REUSE_WINDOW_MS);
    assert!(!t.changed_marker(1, 2, 1_000 + REUSE_WINDOW_MS));
}

#[test]
fn the_same_pane_coming_back_is_not_a_change() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(1, 1), (2, 2)], 0);
    layout(&mut t, 1, &[(1, 1)], 1_000);
    layout(&mut t, 1, &[(1, 1), (2, 2)], 2_000);
    assert!(!t.changed_marker(1, 2, 2_000));
}

#[test]
fn a_pane_that_moves_leaves_its_old_position_to_the_next_one() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(1, 1), (2, 2)], 0);
    // Pane 2 moves to position 1 and a new pane 3 takes position 2.
    layout(&mut t, 1, &[(2, 1), (3, 2)], 1_000);
    assert!(t.changed_marker(1, 1, 1_000), "position 1 changed hands");
    assert!(t.changed_marker(1, 2, 1_000), "position 2 changed hands");
    assert_eq!(t.label(1, 2).map(|l| (l.window, l.index)), Some((1, 1)));
}

#[test]
fn reapplying_the_same_layout_does_not_clear_or_restart_the_marker() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(1, 1), (2, 2)], 0);
    layout(&mut t, 1, &[(1, 1)], 1_000);
    layout(&mut t, 1, &[(1, 1), (3, 2)], 2_000);
    layout(&mut t, 1, &[(1, 1), (3, 2)], 3_000);
    assert!(t.changed_marker(1, 2, 2_000 + MARKER_MS - 1));
    assert!(
        !t.changed_marker(1, 2, 2_000 + MARKER_MS),
        "measured from the reuse, not the last layout"
    );
}

#[test]
fn labels_come_in_index_order_for_the_visible_panes_only() {
    let mut t = Identity::new();
    layout(&mut t, 1, &[(5, 2), (1, 1), (9, 3)], 0);
    layout(&mut t, 1, &[(5, 2), (1, 1)], 10);
    let order: Vec<(u32, u32)> = t.labels(1).iter().map(|l| (l.index, l.id)).collect();
    assert_eq!(order, [(1, 1), (2, 5)]);
    assert_eq!(t.label(1, 9), None, "a hidden pane has no label");
}
