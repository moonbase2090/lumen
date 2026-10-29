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
    assert_eq!(corpus.revision, "pc-1");
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
    assert_eq!(cases.len(), 12);
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
