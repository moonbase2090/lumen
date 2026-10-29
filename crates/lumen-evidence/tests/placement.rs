//! Placement mode over the committed pane corpus.

use lumen_evidence::panes::load_panes;
use lumen_evidence::placement::{CaseResult, Placement, run_placement};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-panes")
}

fn run(p: Placement) -> Vec<CaseResult> {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pane corpus loads")
    };
    assert_eq!(corpus.revision, "pc-2");
    let Ok((report, _)) = run_placement(&corpus, "testsha", p, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.violations(), 0);
    report.cases
}

fn drawn(cases: &[CaseResult], id: &str) -> u32 {
    let Some(c) = cases.iter().find(|c| c.id == id) else {
        unreachable!("case {id} present")
    };
    c.drawn
}

#[test]
fn the_committed_pane_corpus_is_what_the_generator_prints() {
    let committed = std::fs::read_to_string(dir().join("pane-corpus-1.json"));
    assert_eq!(committed.ok(), Some(lumen_evidence::pc1::pc1_json()));
}

#[test]
fn option_a_draws_what_the_rules_allow_and_nothing_else() {
    let cases = run(Placement::A);
    let want = [
        ("two-panes-basic", 2),
        ("output-before-any-layout", 0),
        ("frame-split-across-reads", 1),
        ("stale-layout-after-newer", 1),
        ("equal-generation-layout-ignored", 1),
        ("malformed-rectangles", 0),
        ("pane-not-in-layout", 0),
        ("resize-collapses-until-new-layout", 2),
        ("zoom-and-restore", 2),
        ("detach-and-reattach", 2),
        ("pane-writes-sealed-looking-frame", 0),
        ("forged-frame-among-real-ones", 2),
        ("cross-pane-forged-container-then-replay", 1),
        ("forged-layout-from-a-pane-replayed", 0),
        ("hostile-stale-and-early-containers", 1),
        ("hostile-wrong-rectangle", 0),
        ("hostile-container-for-unlisted-pane", 0),
        ("hostile-container-with-a-sealed-content-type", 0),
        ("hostile-replayed-container", 1),
        ("hostile-frame-type-outside-the-keys", 0),
    ];
    assert_eq!(cases.len(), want.len());
    for (id, n) in want {
        assert_eq!(drawn(&cases, id), n, "{id}");
    }
}

#[test]
fn option_a_sees_the_forged_frames_and_draws_none_of_them() {
    let cases = run(Placement::A);
    let seen = |id: &str| {
        cases
            .iter()
            .find(|c| c.id == id)
            .map_or(0, |c| c.forged_seen)
    };
    assert_eq!(seen("pane-writes-sealed-looking-frame"), 3);
    assert_eq!(seen("forged-frame-among-real-ones"), 1);
    assert_eq!(seen("two-panes-basic"), 0);
}

#[test]
fn option_a_counts_the_malformed_layouts_as_failed_frames() {
    let cases = run(Placement::A);
    let Some(c) = cases.iter().find(|c| c.id == "malformed-rectangles") else {
        unreachable!("case present")
    };
    assert!(c.terminal.contains("malformed: 4"), "{}", c.terminal);
    assert!(c.terminal.contains("failed_frames: 4"), "{}", c.terminal);
}

#[test]
fn option_b_has_no_per_pane_containers_and_no_forged_container() {
    let cases = run(Placement::B);
    assert_eq!(cases.len(), 20);
    for c in &cases {
        assert_eq!(c.drawn, 0, "{}", c.id);
        assert!(c.violations.is_empty(), "{}", c.id);
    }
}

fn counters<'a>(cases: &'a [CaseResult], id: &str) -> &'a str {
    cases
        .iter()
        .find(|c| c.id == id)
        .map_or("", |c| c.terminal.as_str())
}

#[test]
fn forged_frames_from_a_pane_never_reach_the_terminal_as_frames() {
    let cases = run(Placement::A);
    for id in [
        "pane-writes-sealed-looking-frame",
        "forged-frame-among-real-ones",
    ] {
        let c = counters(&cases, id);
        assert!(c.contains("malformed: 0"), "{id}: {c}");
        assert!(c.contains("failed_frames: 0"), "{id}: {c}");
        assert!(c.contains("refused: 0"), "{id}: {c}");
    }
}

#[test]
fn a_resize_leaves_the_multiplexer_without_a_layout_until_it_sends_one() {
    let cases = run(Placement::A);
    let c = counters(&cases, "resize-collapses-until-new-layout");
    assert!(c.contains("resets: 1"), "{c}");
    assert!(c.contains("containers_early: 0"), "{c}");
}

#[test]
fn a_reattach_starts_a_new_layer_with_no_generation() {
    let cases = run(Placement::A);
    let c = counters(&cases, "detach-and-reattach");
    assert!(c.contains("layouts_accepted: 1"), "{c}");
    assert!(c.contains("layouts_stale: 0"), "{c}");
    assert!(c.contains("refused: 0"), "fresh keys on both sides: {c}");
    assert!(c.contains("failed_frames: 0"), "{c}");
}

fn total(cases: &[CaseResult], f: impl Fn(&CaseResult) -> u32) -> u32 {
    cases.iter().map(f).sum()
}

#[test]
fn a_hostile_multiplexer_cannot_get_an_out_of_generation_container_drawn() {
    let cases = run(Placement::A);
    assert_eq!(total(&cases, |c| c.out_of_generation_sent), 3);
    assert_eq!(total(&cases, |c| c.out_of_generation_dropped), 3);
}

#[test]
fn a_replayed_frame_is_refused_every_time() {
    let cases = run(Placement::A);
    // One hostile case replays a container twice; the cross-pane cases replay
    // the last frame the terminal took (a layout) three times.
    assert_eq!(total(&cases, |c| c.replays_sent), 5);
    assert_eq!(total(&cases, |c| c.replays_refused), 5);
}

#[test]
fn pane_written_sealed_looking_frames_do_not_open_even_if_passed_through() {
    let cases = run(Placement::A);
    assert_eq!(total(&cases, |c| c.forged_seen), 6);
    assert_eq!(total(&cases, |c| c.passthrough_opened), 0);
}

#[test]
fn the_hostile_cases_exercise_the_terminal_counters_they_are_for() {
    let cases = run(Placement::A);
    let stale = counters(&cases, "hostile-stale-and-early-containers");
    assert!(stale.contains("containers_stale: 1"), "{stale}");
    assert!(stale.contains("containers_early: 1"), "{stale}");
    let rect = counters(&cases, "hostile-wrong-rectangle");
    assert!(rect.contains("containers_dropped: 2"), "{rect}");
    assert!(rect.contains("malformed: 2"), "{rect}");
    let foreign = counters(&cases, "hostile-frame-type-outside-the-keys");
    assert!(foreign.contains("other_type: 1"), "{foreign}");
    assert!(
        foreign.contains("containers_early: 0"),
        "ignored once desynchronised: {foreign}"
    );
    let kind = counters(&cases, "hostile-container-with-a-sealed-content-type");
    assert!(kind.contains("malformed: 1"), "{kind}");
    assert!(kind.contains("containers_unparsed: 1"), "{kind}");
}

#[test]
fn option_b_counts_the_forged_frames_and_replays_instead_of_reading_zero_of_zero() {
    let cases = run(Placement::B);
    let get = |id: &str| {
        let Some(c) = cases.iter().find(|c| c.id == id) else {
            unreachable!("case {id} present")
        };
        (
            c.forged_seen,
            c.passthrough_opened,
            c.replays_sent,
            c.replays_refused,
        )
    };
    // Seen, none opened, every replay refused.
    assert_eq!(get("pane-writes-sealed-looking-frame"), (3, 0, 0, 0));
    assert_eq!(get("cross-pane-forged-container-then-replay"), (1, 0, 2, 2));
    assert_eq!(get("forged-layout-from-a-pane-replayed"), (1, 0, 1, 1));
    assert_eq!(total(&cases, |c| c.passthrough_opened), 0);
    assert!(total(&cases, |c| c.forged_seen) >= 6);
    assert!(total(&cases, |c| c.replays_sent) >= 3);
}

#[test]
fn both_placements_see_the_new_cross_pane_cases_without_a_violation() {
    for p in [Placement::A, Placement::B] {
        let cases = run(p);
        let seen = |id: &str| {
            cases
                .iter()
                .find(|c| c.id == id)
                .map_or(0, |c| c.forged_seen)
        };
        assert_eq!(seen("cross-pane-forged-container-then-replay"), 1, "{p:?}");
        assert_eq!(seen("forged-layout-from-a-pane-replayed"), 1, "{p:?}");
    }
}
