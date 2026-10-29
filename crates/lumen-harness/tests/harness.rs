//! Recorder lines and effect-log verdicts.

use lumen_harness::{
    Effect, EffectLog, HeaderInfo, MAX_LEAKS_KEPT, Origin, Recorder, SinkTable, TerminalSinks,
    Verdict,
};

fn header() -> HeaderInfo {
    HeaderInfo {
        commit: "abc123".into(),
        features: "arm-b".into(),
        harness: "h1".into(),
        corpus: "r1".into(),
        session_matrix: "none".into(),
        sinks: "none".into(),
    }
}

fn lines(bytes: Vec<u8>) -> Vec<String> {
    String::from_utf8(bytes)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn recorder_writes_a_header_then_one_line_per_read() {
    let Ok(mut r) = Recorder::new(Vec::new(), &header()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(r.read(5, 100, &[("output", 90), ("block", 10)]).is_ok());
    assert!(r.read(9, 0, &[]).is_ok());
    let Ok(out) = r.finish() else {
        unreachable!("flushing a Vec succeeds")
    };
    assert_eq!(
        lines(out),
        vec![
            r#"{"header":{"format":2,"commit":"abc123","features":"arm-b","harness":"h1","corpus":"r1","session_matrix":"none","sinks":"none"}}"#,
            r#"{"t_ms":5,"bytes_in":100,"items":[{"kind":"output","len":90},{"kind":"block","len":10}]}"#,
            r#"{"t_ms":9,"bytes_in":0,"items":[]}"#,
        ]
    );
}

#[test]
fn header_values_are_escaped_and_cannot_break_the_line() {
    let h = HeaderInfo {
        commit: "a\"b\\c\nd\u{1}".into(),
        features: String::new(),
        harness: "h1".into(),
        corpus: "x".into(),
        session_matrix: "none".into(),
        sinks: "none".into(),
    };
    let Ok(r) = Recorder::new(Vec::new(), &h) else {
        unreachable!("writing to a Vec succeeds")
    };
    let Ok(out) = r.finish() else {
        unreachable!("flushing a Vec succeeds")
    };
    let l = lines(out);
    assert_eq!(l.len(), 1, "one line only: {l:?}");
    assert!(l[0].contains(r#""commit":"a\"b\\c\nd\u0001""#), "{}", l[0]);
}

const SINKS: [(Effect, &str); 6] = [
    (Effect::ClipboardWrite, "term::clipboard"),
    (Effect::Notification, "term::notify"),
    (Effect::LinkOpen, "term::link"),
    (Effect::Grant, "term::grant"),
    (Effect::ExitStatus, "term::exit"),
    (Effect::BoundaryChange, "term::boundary"),
];

fn real_sink(effect: Effect) -> &'static str {
    SINKS
        .iter()
        .find(|(e, _)| *e == effect)
        .map_or("?", |(_, id)| *id)
}

/// A table for these tests only. In real use the terminal's effect code
/// implements `TerminalSinks`; the harness has no way to declare one.
struct TestTerminal;

impl TerminalSinks for TestTerminal {
    fn source(&self) -> &'static str {
        "test-only"
    }

    fn sinks(&self) -> &'static [(Effect, &'static str)] {
        &SINKS
    }
}

fn terminal_log() -> EffectLog {
    EffectLog::new(SinkTable::from_terminal(&TestTerminal))
}

#[test]
fn the_table_source_is_visible_for_the_header() {
    assert_eq!(terminal_log().table_source(), "test-only");
    assert_eq!(EffectLog::new(SinkTable::none()).table_source(), "none");
}

fn exercise_all(log: &mut EffectLog) {
    for e in Effect::ALL {
        log.attempt(e, Origin::Sealed, "in", real_sink(e));
    }
}

#[test]
fn with_no_terminal_sinks_registered_nothing_is_measured() {
    let mut log = EffectLog::new(SinkTable::none());
    for e in Effect::ALL {
        log.attempt(e, Origin::Sealed, "in", "term::whatever");
    }
    assert!(matches!(log.verdict(), Verdict::NotMeasured(_)));
    assert!(log.report().contains("2.4=not measured"));
}

#[test]
fn a_stub_driver_cannot_produce_a_zero_even_if_it_exercises_every_sink() {
    let mut log = terminal_log();
    for e in Effect::ALL {
        log.attempt(e, Origin::Sealed, "in", "harness::StubSink");
    }
    match log.verdict() {
        Verdict::NotMeasured(why) => assert!(why.contains("harness::StubSink"), "{why}"),
        other => assert_eq!(format!("{other:?}"), "NotMeasured"),
    }
}

#[test]
fn one_stub_among_real_sinks_makes_the_run_not_measured() {
    let mut log = terminal_log();
    exercise_all(&mut log);
    assert_eq!(log.verdict(), Verdict::Zero);
    log.attempt(Effect::Grant, Origin::Sealed, "in", "harness::StubSink");
    assert!(matches!(log.verdict(), Verdict::NotMeasured(_)));
    assert!(
        log.report()
            .contains("grant exercised=2 unsealed=0 foreign=1")
    );
}

#[test]
fn a_stub_cannot_hide_a_leak_it_is_not_reported_as_a_terminal_leak() {
    // A leak seen only through a stub is not a terminal result either way.
    let mut log = terminal_log();
    exercise_all(&mut log);
    log.attempt(
        Effect::ClipboardWrite,
        Origin::Unsealed,
        "x",
        "harness::StubSink",
    );
    assert_eq!(log.leak_count(), 0);
    assert!(matches!(log.verdict(), Verdict::NotMeasured(_)));
}

#[test]
fn a_zero_needs_every_sink_exercised() {
    let mut log = terminal_log();
    for e in &Effect::ALL[..5] {
        log.attempt(*e, Origin::Sealed, "in", real_sink(*e));
    }
    match log.verdict() {
        Verdict::NotMeasured(why) => assert!(why.contains("boundary_change"), "{why}"),
        other => assert_eq!(format!("{other:?}"), "NotMeasured"),
    }
    log.attempt(
        Effect::BoundaryChange,
        Origin::Sealed,
        "in",
        real_sink(Effect::BoundaryChange),
    );
    assert_eq!(log.verdict(), Verdict::Zero);
}

#[test]
fn an_unsealed_attempt_through_a_real_sink_is_non_zero_with_its_input() {
    let mut log = terminal_log();
    exercise_all(&mut log);
    log.attempt(
        Effect::ClipboardWrite,
        Origin::Unsealed,
        "corpus/osc52-7",
        real_sink(Effect::ClipboardWrite),
    );
    assert_eq!(log.verdict(), Verdict::NonZero(1));
    assert_eq!(log.leaks()[0].input, "corpus/osc52-7");
    let clipboard = &log.sinks()[0];
    assert_eq!((clipboard.exercised, clipboard.unsealed), (2, 1));
}

#[test]
fn the_leak_list_is_capped_but_the_count_is_exact() {
    let mut log = terminal_log();
    for i in 0..(MAX_LEAKS_KEPT + 500) {
        log.attempt(
            Effect::Notification,
            Origin::Unsealed,
            &format!("in{i}"),
            real_sink(Effect::Notification),
        );
    }
    assert_eq!(log.leaks().len(), MAX_LEAKS_KEPT);
    assert_eq!(log.leak_count(), MAX_LEAKS_KEPT + 500);
    assert_eq!(log.verdict(), Verdict::NonZero(MAX_LEAKS_KEPT + 500));
}

#[test]
fn the_report_names_each_sinks_exercised_count() {
    let mut log = terminal_log();
    log.attempt(
        Effect::LinkOpen,
        Origin::Sealed,
        "a",
        real_sink(Effect::LinkOpen),
    );
    log.attempt(
        Effect::LinkOpen,
        Origin::Sealed,
        "b",
        real_sink(Effect::LinkOpen),
    );
    let report = log.report();
    assert!(
        report.contains("link_open exercised=2 unsealed=0 foreign=0"),
        "{report}"
    );
    assert!(
        report.contains("grant exercised=0 unsealed=0 foreign=0"),
        "{report}"
    );
    assert_eq!(report.lines().count(), 1 + Effect::ALL.len());
}

#[test]
fn recorder_marks_case_boundaries() {
    let Ok(mut r) = Recorder::new(Vec::new(), &header()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(r.case("a\"b").is_ok());
    assert!(r.read(0, 1, &[]).is_ok());
    let Ok(out) = r.finish() else {
        unreachable!("flushing a Vec succeeds")
    };
    let got = lines(out);
    assert_eq!(got[1], r#"{"case":"a\"b"}"#);
    assert_eq!(got.len(), 3);
}
