//! Recorder lines and effect-log verdicts.

use lumen_harness::{Effect, EffectLog, HeaderInfo, Level, Origin, Recorder, Verdict};

fn header() -> HeaderInfo {
    HeaderInfo {
        commit: "abc123".into(),
        features: "arm-b".into(),
        corpus: "r1".into(),
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
            r#"{"header":{"format":1,"commit":"abc123","features":"arm-b","corpus":"r1"}}"#,
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
        corpus: "x".into(),
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

#[test]
fn a_stub_run_is_never_a_measurement() {
    let mut log = EffectLog::new(Level::Stub);
    for e in Effect::ALL {
        log.attempt(e, Origin::Sealed, "in");
    }
    assert!(matches!(log.verdict(), Verdict::NotMeasured(_)));
    assert!(log.report().contains("2.4=not measured"));
}

#[test]
fn a_zero_needs_every_sink_exercised() {
    let mut log = EffectLog::new(Level::Terminal);
    for e in &Effect::ALL[..5] {
        log.attempt(*e, Origin::Sealed, "in");
    }
    match log.verdict() {
        Verdict::NotMeasured(why) => assert!(why.contains("boundary_change"), "{why}"),
        other => panic_free(&format!("{other:?}")),
    }
    log.attempt(Effect::BoundaryChange, Origin::Sealed, "in");
    assert_eq!(log.verdict(), Verdict::Zero);
}

fn panic_free(msg: &str) {
    assert!(msg.is_empty(), "unexpected verdict {msg}");
}

#[test]
fn an_unsealed_attempt_is_non_zero_and_recorded_with_its_input() {
    let mut log = EffectLog::new(Level::Terminal);
    for e in Effect::ALL {
        log.attempt(e, Origin::Sealed, "ok");
    }
    log.attempt(Effect::ClipboardWrite, Origin::Unsealed, "corpus/osc52-7");
    assert_eq!(log.verdict(), Verdict::NonZero(1));
    assert_eq!(log.leaks()[0].input, "corpus/osc52-7");
    let clipboard = &log.sinks()[0];
    assert_eq!((clipboard.exercised, clipboard.unsealed), (2, 1));
}

#[test]
fn the_report_names_each_sinks_exercised_count() {
    let mut log = EffectLog::new(Level::Terminal);
    log.attempt(Effect::LinkOpen, Origin::Sealed, "a");
    log.attempt(Effect::LinkOpen, Origin::Sealed, "b");
    let report = log.report();
    assert!(
        report.contains("link_open exercised=2 unsealed=0"),
        "{report}"
    );
    assert!(report.contains("grant exercised=0 unsealed=0"), "{report}");
    assert_eq!(report.lines().count(), 2 + Effect::ALL.len());
}
