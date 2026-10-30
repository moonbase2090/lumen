//! The pane process-trust corpus and its Option A process-state run.

use lumen_evidence::panes::{Classification, Event, ProcessEvent, load_panes, parse_file};
use lumen_evidence::placement::{CaseResult, Placement, Report, run_placement};
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-pane-process")
}

fn run(placement: Placement) -> Report {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    let Ok((report, _)) = run_placement(&corpus, "t", placement, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    report
}

fn case<'a>(report: &'a Report, id: &str) -> &'a CaseResult {
    let Some(case) = report.cases.iter().find(|case| case.id == id) else {
        unreachable!("case {id} is present")
    };
    case
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
fn option_a_plays_every_process_case_and_checks_every_expectation() {
    let Ok(corpus) = load_panes(&dir()) else {
        unreachable!("pp-1 loads")
    };
    let Ok((report, header)) = run_placement(&corpus, "t", Placement::A, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.not_run(), 0);
    assert_eq!(report.violations(), 0);
    assert_eq!(
        report
            .cases
            .iter()
            .map(|case| case.process_expectations)
            .sum::<u32>(),
        12
    );
    assert_eq!(
        report
            .cases
            .iter()
            .map(|case| case.classified_reads)
            .sum::<u32>(),
        12
    );
    let text = report.summary(&header);
    assert!(
        text.contains(
            "pane_process_trust=pass (12 expectations, 12 output reads classified, 0 failures)"
        ),
        "{text}"
    );
    assert!(!text.contains("pane_process_trust=not measured"), "{text}");
}

#[test]
fn option_b_still_reports_process_cases_as_not_measured() {
    let report = run(Placement::B);
    assert_eq!(report.not_run(), 8);
    assert_eq!(report.violations(), 0);
    assert!(
        report
            .cases
            .iter()
            .all(|case| case.drawn == 0 && case.terminal.is_empty()),
        "nothing was played"
    );
}

#[test]
fn option_a_records_each_sticky_process_fact() {
    let report = run(Placement::A);
    let state = |id: &str| {
        let Some(state) = case(&report, id).process_states.get(1) else {
            unreachable!("{id} records pane 1")
        };
        state
    };

    assert!(!state("first-command-not-a-listed-shell").listed_shell());

    let clean = state("listed-shell-with-every-condition-holding");
    assert!(clean.listed_shell());
    assert!(!clean.exec_seen());
    assert!(!clean.pid_reuse_seen());
    assert!(!clean.job_control_lost());
    assert!(!clean.wide_foreground_seen());

    let exec = state("exec-in-the-pane-shell");
    assert!(exec.exec_seen());
    assert_eq!(exec.observed().program().as_path(), Path::new("/bin/zsh"));

    assert!(state("recorded-pid-reused-by-another-process").pid_reuse_seen());

    let job_control = state("job-control-off");
    assert!(job_control.job_control_lost());
    assert_eq!(job_control.job_control(), Some(true));

    let group = state("second-process-in-the-foreground-group");
    assert!(group.wide_foreground_seen());
    assert_eq!(
        group.foreground_members().map(std::num::NonZeroU32::get),
        Some(1)
    );

    let foreground = state("frame-read-while-ssh-was-in-the-foreground");
    assert_eq!(
        foreground
            .foreground_program()
            .map(lumen_panes::Program::as_path),
        Some(Path::new("/bin/zsh"))
    );
}

#[test]
fn a_local_expectation_fails_the_v0_pane_model() {
    let text = r#"{"revision":"x","cases":[{"id":"local","category":"process-trust","screen":{"cols":80,"rows":24},"script":[
        {"op":"process","pane":1,"what":"spawn","program":"/bin/zsh","listed":true},
        {"op":"expect","pane":1,"track0":"local"}]}]}"#;
    let Ok((_, cases)) = parse_file(text) else {
        unreachable!("case parses")
    };
    let corpus = lumen_evidence::panes::PaneCorpus {
        revision: "x".into(),
        cases,
    };
    let Ok((report, _)) = run_placement(&corpus, "t", Placement::A, Vec::new()) else {
        unreachable!("runs")
    };
    assert_eq!(report.violations(), 1);
    assert_eq!(report.cases[0].process_failures, 1);
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
    for (placement, not_run) in [(Placement::A, 0), (Placement::B, 1)] {
        let Ok((report, _)) = run_placement(&corpus, "t", placement, Vec::new()) else {
            unreachable!("runs")
        };
        assert_eq!(report.not_run(), not_run);
        assert!(report.cases[0].not_run.is_none());
        assert_eq!(report.cases[1].not_run.is_some(), placement == Placement::B);
    }
}

#[test]
fn the_binary_measures_process_cases_only_under_option_a() {
    let run = |corpus: &Path, placement: &str| {
        Command::new(env!("CARGO_BIN_EXE_lumen-evidence"))
            .args(["--panes"])
            .arg(corpus)
            .args(["--placement", placement, "--commit", "t", "--out"])
            .arg(Path::new(env!("CARGO_TARGET_TMPDIR")).join("pp-run"))
            .output()
    };
    let Ok(measured) = run(&dir(), "a") else {
        unreachable!("runs")
    };
    assert_eq!(measured.status.code(), Some(0));
    let Ok(none) = run(&dir(), "b") else {
        unreachable!("runs")
    };
    assert_eq!(none.status.code(), Some(3));
    let some = run(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-panes"),
        "a",
    );
    assert_eq!(some.ok().and_then(|o| o.status.code()), Some(0));
}
