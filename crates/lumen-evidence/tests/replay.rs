//! Replays the fixture corpus through the driver.

use lumen_evidence::{Arm, load_corpus, run, run_arm};
use std::path::Path;

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
}

#[test]
fn the_fixture_corpus_replays_with_the_expected_items() {
    let Ok(corpus) = load_corpus(&fixture()) else {
        unreachable!("fixture loads")
    };
    assert_eq!(corpus.revision, "fixture-1");
    let Ok((report, header)) = run(&corpus, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    let get = |id: &str| report.cases.iter().find(|c| c.id == id);
    let Some(plain) = get("para-plain") else {
        unreachable!("case present")
    };
    assert_eq!(plain.kinds.get("block"), Some(&1));
    assert_eq!(plain.violations, 0);
    let Some(alt) = get("para-alt-screen") else {
        unreachable!("case present")
    };
    assert_eq!(alt.kinds.get("text"), Some(&1));
    assert_eq!(alt.kinds.get("block"), None);
    let Some(forged) = get("forged-sealed-type") else {
        unreachable!("case present")
    };
    assert_eq!(forged.kinds.get("sealed"), None);
    assert_eq!(forged.violations, 0);
    let Some(split) = get("split-frame") else {
        unreachable!("case present")
    };
    assert_eq!(split.kinds.get("block"), Some(&1));
    // The one case that forbids what it produces is the only violation.
    assert_eq!(report.violations(), 1);
    assert_eq!(header.sinks, "lumen-sinks");
    assert!(report.authority.starts_with("2.4=not measured"));
}

#[test]
fn the_log_has_a_header_and_case_markers() {
    let Ok(corpus) = load_corpus(&fixture()) else {
        unreachable!("fixture loads")
    };
    let mut log = Vec::new();
    assert!(run(&corpus, "testsha", &mut log).is_ok());
    let text = String::from_utf8(log).unwrap_or_default();
    let lines: Vec<_> = text.lines().collect();
    assert!(lines[0].contains("\"commit\":\"testsha\""));
    assert!(lines[0].contains("\"corpus\":\"fixture-1\""));
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("{\"case\"")).count(),
        5
    );
}

fn write_corpus(name: &str, forbid: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lumen-evidence-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = format!(
        r#"{{"revision":"t","cases":[{{"id":"case-x","reads":[{{"hex":"61"}}],"forbid":{forbid}}}]}}"#
    );
    assert!(std::fs::write(dir.join("c.json"), doc).is_ok());
    dir
}

#[test]
fn a_misspelled_forbid_kind_fails_the_load_with_the_case_id() {
    for bad in [
        r#"["Sealed"]"#,
        r#"["blocks"]"#,
        r#"["sealed", "nope"]"#,
        "[1]",
    ] {
        let dir = write_corpus("typo", bad);
        let Err(e) = load_corpus(&dir) else {
            unreachable!("{bad} must not load")
        };
        assert!(e.contains("case-x"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    let dir = write_corpus("ok", r#"["sealed", "block"]"#);
    assert!(load_corpus(&dir).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

fn replay(arm: Arm) -> (lumen_evidence::Report, lumen_harness::HeaderInfo) {
    let Ok(corpus) = load_corpus(&fixture()) else {
        unreachable!("fixture loads")
    };
    let Ok(out) = run_arm(&corpus, "testsha", arm, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    out
}

fn kinds(
    report: &lumen_evidence::Report,
    id: &str,
) -> std::collections::BTreeMap<&'static str, usize> {
    report
        .cases
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.kinds.clone())
        .unwrap_or_default()
}

#[test]
fn arm_a_on_a_sealed_layer_draws_track0_in_the_container() {
    let (report, header) = replay(Arm::ASealed);
    assert_eq!(header.features, "arm-a sealed-layer");
    assert_eq!(kinds(&report, "para-plain").get("block"), Some(&1));
    assert_eq!(kinds(&report, "split-frame").get("block"), Some(&1));
    // Alternate screen and a forged sealed type stay out of blocks and sealed items.
    assert_eq!(kinds(&report, "para-alt-screen").get("block"), None);
    assert_eq!(kinds(&report, "forged-sealed-type").get("sealed"), None);
    // Only the case that forbids what it produces is a violation.
    assert_eq!(report.violations(), 1);
}

#[test]
fn arm_a_on_an_unsealed_layer_draws_nothing_rich() {
    let (report, header) = replay(Arm::AUnsealed);
    assert_eq!(header.features, "arm-a unsealed-layer");
    for case in &report.cases {
        assert_eq!(case.kinds.get("block"), None, "{}", case.id);
        assert_eq!(case.kinds.get("sealed"), None, "{}", case.id);
    }
    assert_eq!(kinds(&report, "para-plain").get("text"), Some(&1));
    assert_eq!(report.violations(), 0);
}

#[test]
fn both_arms_name_their_counters_the_same_way() {
    let (a, _) = replay(Arm::ASealed);
    let (b, _) = replay(Arm::B);
    let names = |s: &str| {
        s.split(['{', '}', ','])
            .filter_map(|p| p.split(':').next().map(str::trim))
            .skip(1)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&a.cases[0].counters), names(&b.cases[0].counters));
    assert!(names(&a.cases[0].counters).len() >= 17);
}

fn rt2_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-rt-2")
}

#[test]
fn the_committed_rt2_corpus_is_what_the_generator_prints() {
    let committed = std::fs::read_to_string(rt2_dir().join("red-team-2.json"));
    assert_eq!(committed.ok(), Some(lumen_evidence::rt2::rt2_json()));
}

fn rt2_counters(arm: Arm, id: &str) -> String {
    let Ok(corpus) = load_corpus(&rt2_dir()) else {
        unreachable!("rt-2 loads")
    };
    let Ok((report, _)) = run_arm(&corpus, "testsha", arm, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.violations(), 0);
    let Some(c) = report.cases.iter().find(|c| c.id == id) else {
        unreachable!("case {id} present")
    };
    c.counters.clone()
}

#[test]
fn rt2_real_sealed_frames_are_opened_by_both_arms_on_a_sealed_layer() {
    for arm in [Arm::B, Arm::ASealed] {
        assert!(rt2_counters(arm, "sealed-valid").contains("sealed_opened: 1,"));
        assert!(rt2_counters(arm, "sealed-valid-two-in-order").contains("sealed_opened: 2,"));
        assert!(rt2_counters(arm, "sealed-valid-split-across-reads").contains("sealed_opened: 1,"));
        assert!(
            rt2_counters(arm, "sealed-over-body-cap-in-one-second").contains("sealed_opened: 256,")
        );
    }
}

#[test]
fn rt2_refused_frames_are_counted_and_never_opened() {
    for arm in [Arm::B, Arm::ASealed] {
        let c = rt2_counters(arm, "sealed-tampered-body");
        assert!(c.contains("body_aead_failures: 1,") && c.contains("sealed_opened: 0,"));
        assert!(rt2_counters(arm, "sealed-replayed-frame").contains("refused_replay: 1,"));
        assert!(
            rt2_counters(arm, "sealed-over-body-cap-in-one-second")
                .contains("refused_over_body_cap: 44,")
        );
        assert!(rt2_counters(arm, "sealed-aborts-pin-the-layer").contains("pinned_notices: 1,"));
        assert!(rt2_counters(arm, "sealed-abort-mid-body").contains("truncated: 1,"));
        assert!(rt2_counters(arm, "sealed-wrong-keys").contains("sealed_opened: 0,"));
    }
}

#[test]
fn rt2_opens_nothing_on_an_unsealed_layer() {
    for id in [
        "sealed-valid",
        "sealed-valid-two-in-order",
        "sealed-over-body-cap-in-one-second",
    ] {
        assert!(rt2_counters(Arm::AUnsealed, id).contains("sealed_opened: 0,"));
    }
}

fn rt3() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-rt-3")
}

#[test]
fn the_notification_corpus_is_denied_before_the_sink_in_every_arm() {
    let Ok(corpus) = load_corpus(&rt3()) else {
        unreachable!("the committed corpus loads")
    };
    assert_eq!(corpus.revision, "rt-3");
    for arm in [Arm::B, Arm::ASealed, Arm::AUnsealed] {
        let Ok((report, _)) = run_arm(&corpus, "testsha", arm, Vec::new()) else {
            unreachable!("writing to a Vec succeeds")
        };
        assert_eq!(report.violations(), 0, "{arm:?}");
        let attempts: u64 = report
            .cases
            .iter()
            .filter_map(|c| c.sinks.as_ref().map(|s| s.notification_attempts))
            .sum();
        let denied: u64 = report
            .cases
            .iter()
            .filter_map(|c| c.sinks.as_ref().map(|s| s.denied_notifications))
            .sum();
        assert_eq!(
            attempts, 0,
            "{arm:?}: no unsigned notification reaches the sink"
        );
        assert!(denied >= 50, "{arm:?}: denials are counted ({denied})");
        // Nothing was raised, and five sinks have no code, so 2.4 stays not measured.
        assert!(
            report.authority.starts_with("2.4=not measured"),
            "{}",
            report.authority
        );
    }
}

#[test]
fn a_sinks_expectation_that_does_not_hold_is_a_violation() {
    let dir = std::env::temp_dir().join(format!("lumen-evidence-sinks-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    // The case denies one notification but expects two.
    let doc = r#"{"revision":"t","cases":[
        {"id":"wrong","reads":[{"hex":"1b5d393b7807"}],"sinks":{"denied_notifications":2}},
        {"id":"right","reads":[{"hex":"1b5d393b7807"}],"sinks":{"denied_notifications":1,"notification_attempts":0}}]}"#;
    assert!(std::fs::write(dir.join("c.json"), doc).is_ok());
    let Ok(corpus) = load_corpus(&dir) else {
        unreachable!("the corpus loads")
    };
    let Ok((report, _)) = run_arm(&corpus, "testsha", Arm::B, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    let violations: Vec<_> = report
        .cases
        .iter()
        .map(|c| (c.id.as_str(), c.violations))
        .collect();
    assert_eq!(violations, [("wrong", 1), ("right", 0)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_sinks_object_fails_the_load_with_the_case_id() {
    for (i, bad) in [
        r#""sinks":{"nope":1}"#,
        r#""sinks":{"denied_notifications":"1"}"#,
        r#""sinks":[1]"#,
    ]
    .iter()
    .enumerate()
    {
        let dir =
            std::env::temp_dir().join(format!("lumen-evidence-sinksbad{i}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let doc = format!(
            r#"{{"revision":"t","cases":[{{"id":"case-x","reads":[{{"hex":"61"}}],{bad}}}]}}"#
        );
        assert!(std::fs::write(dir.join("c.json"), doc).is_ok());
        let e = load_corpus(&dir).err().unwrap_or_default();
        assert!(e.contains("case-x") && e.contains("sinks"), "{bad}: {e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn rt4_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-rt-4")
}

#[test]
fn the_committed_rt4_corpus_is_what_the_generator_prints() {
    if std::env::var_os("LUMEN_REGENERATE_RT4").is_some() {
        let _ = std::fs::create_dir_all(rt4_dir());
        assert!(
            std::fs::write(
                rt4_dir().join("sealed-sinks.json"),
                lumen_evidence::rt4::rt4_json()
            )
            .is_ok()
        );
    }
    let committed = std::fs::read_to_string(rt4_dir().join("sealed-sinks.json"));
    assert_eq!(committed.ok(), Some(lumen_evidence::rt4::rt4_json()));
}
