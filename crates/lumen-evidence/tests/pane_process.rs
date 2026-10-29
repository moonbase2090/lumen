//! The pane process-trust corpus: its shape, and that no run mistakes it for a measurement.

use lumen_evidence::panes::{Classification, Event, ProcessEvent, load_panes, parse_file};
use lumen_evidence::placement::{Placement, run_placement};
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-pane-process")
}

#[test]
fn the_committed_file_is_what_the_generator_prints() {
    let committed = std::fs::read_to_string(dir().join("pane-process-1.json"));
    assert_eq!(committed.ok(), Some(lumen_evidence::pp1::pp1_json()));
}

#[test]
fn the_corpus_loads_and_every_case_spawns_a_process_and_expects_something() {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    assert_eq!(corpus.revision, "pp-1");
    assert_eq!(corpus.cases.len(), 8);
    for case in &corpus.cases {
        assert!(
            matches!(
                case.script.iter().find(|e| e.is_process_trust()),
                Some(Event::Process {
                    what: ProcessEvent::Spawn { .. },
                    ..
                })
            ),
            "{} starts by spawning the pane's first command",
            case.id
        );
        assert!(
            case.script
                .iter()
                .any(|e| matches!(e, Event::Expect { .. })),
            "{} has an expectation",
            case.id
        );
    }
}

#[test]
fn no_case_expects_a_pane_to_be_local_because_there_is_no_local_signal_for_panes_in_v0() {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    for case in &corpus.cases {
        for e in &case.script {
            if let Event::Expect { track0, .. } = e {
                assert_eq!(*track0, Classification::PossiblyRemote, "{}", case.id);
            }
        }
    }
}

#[test]
fn the_cases_the_gate_names_are_all_there() {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    let has = |needle: &dyn Fn(&ProcessEvent) -> bool| {
        corpus.cases.iter().any(|c| {
            c.script
                .iter()
                .any(|e| matches!(e, Event::Process { what, .. } if needle(what)))
        })
    };
    assert!(
        has(&|w| matches!(w, ProcessEvent::Spawn { listed: false, .. })),
        "not a listed shell"
    );
    assert!(has(&|w| matches!(w, ProcessEvent::Exec { .. })), "exec");
    assert!(has(&|w| matches!(w, ProcessEvent::PidReuse)), "pid reuse");
    assert!(
        has(&|w| matches!(w, ProcessEvent::JobControl { on: false })),
        "job control off"
    );
    assert!(
        has(&|w| matches!(w, ProcessEvent::Group { members } if *members > 1)),
        "second process"
    );
    assert!(
        has(&|w| matches!(w, ProcessEvent::Foreground { .. })),
        "ssh in the foreground"
    );
    assert!(
        corpus
            .cases
            .iter()
            .any(|c| c.script.iter().any(|e| matches!(
                e,
                Event::Expect {
                    scope: lumen_evidence::panes::Scope::FramesReadEarlier,
                    ..
                }
            ))),
        "a frame read earlier"
    );
}

#[test]
fn placement_mode_does_not_play_them_and_says_so_for_both_placements() {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    for p in [Placement::A, Placement::B] {
        let Ok((report, header)) = run_placement(&corpus, "t", p, Vec::new()) else {
            unreachable!("writing to a Vec succeeds")
        };
        assert_eq!(report.not_run(), 8);
        assert_eq!(report.violations(), 0);
        let text = report.summary(&header);
        assert!(
            text.contains("cases=8 not_run=8 violations=0 of 0 run"),
            "{text}"
        );
        assert!(
            text.contains("pane_process_trust=not measured (8 cases not run"),
            "{text}"
        );
        assert!(
            report
                .cases
                .iter()
                .all(|c| c.drawn == 0 && c.terminal.is_empty()),
            "nothing was played"
        );
    }
}

#[test]
fn a_mixed_corpus_plays_the_ordinary_cases_and_skips_only_the_process_ones() {
    let ordinary = r#"{"id":"plain","category":"t","screen":{"cols":80,"rows":24},"script":[
        {"op":"layout","generation":1,"panes":[{"id":1,"x":0,"y":0,"w":80,"h":24}]}]}"#;
    let process = r#"{"id":"proc","category":"t","screen":{"cols":80,"rows":24},"script":[
        {"op":"process","pane":1,"what":"spawn","program":"/bin/zsh","listed":true},
        {"op":"expect","pane":1,"track0":"possibly_remote"}]}"#;
    let Ok((_, cases)) = parse_file(&format!(
        r#"{{"revision":"x","cases":[{ordinary},{process}]}}"#
    )) else {
        unreachable!("parses")
    };
    let corpus = lumen_evidence::panes::PaneCorpus {
        revision: "x".into(),
        cases,
    };
    let Ok((report, _)) = run_placement(&corpus, "t", Placement::A, Vec::new()) else {
        unreachable!("runs")
    };
    assert_eq!(report.not_run(), 1);
    assert!(report.cases[0].not_run.is_none());
    assert!(report.cases[1].not_run.is_some());
}

#[test]
fn the_binary_exits_3_when_nothing_was_measured_and_0_when_something_was() {
    let run = |corpus: &Path| {
        Command::new(env!("CARGO_BIN_EXE_lumen-evidence"))
            .args(["--panes"])
            .arg(corpus)
            .args(["--placement", "a", "--commit", "t", "--out"])
            .arg(Path::new(env!("CARGO_TARGET_TMPDIR")).join("pp-run"))
            .output()
    };
    let Ok(none) = run(&dir()) else {
        unreachable!("runs")
    };
    assert_eq!(none.status.code(), Some(3));
    let some =
        run(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-panes"));
    assert_eq!(some.ok().and_then(|o| o.status.code()), Some(0));
}
