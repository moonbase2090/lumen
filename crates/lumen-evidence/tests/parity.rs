//! Parity mode: the catalogue rules and the count.

use lumen_evidence::parity::{load_catalogue, run_parity};
use std::path::{Path, PathBuf};

fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/channels.json")
}

fn load_text(name: &str, text: &str) -> Result<lumen_evidence::parity::Catalogue, String> {
    let dir = std::env::temp_dir().join(format!("lumen-parity-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("c.json");
    assert!(std::fs::write(&path, text).is_ok());
    let loaded = load_catalogue(&path);
    let _ = std::fs::remove_dir_all(&dir);
    loaded
}

#[test]
fn the_committed_catalogue_declares_no_channel_so_parity_is_not_measured() {
    let Ok(c) = load_catalogue(&committed()) else {
        unreachable!("the committed catalogue loads")
    };
    let Ok((report, header)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(!report.measured());
    assert_eq!(report.exit_code(), 3, "an empty catalogue is not a pass");
    let s = report.summary(&header);
    assert!(s.contains("2.9=not measured"), "{s}");
    assert!(!s.contains("channels_accepting_more_than_pty"), "{s}");
}

#[test]
fn a_channel_with_an_action_that_has_no_pty_equivalent_is_counted_once() {
    let text = r#"{"revision":"t","channels":[
        {"id":"ok","actions":[{"id":"a","pty_equivalent":"write the same bytes"}]},
        {"id":"more","actions":[
            {"id":"x","pty_equivalent":null},
            {"id":"y","pty_equivalent":null},
            {"id":"z","pty_equivalent":"a plain write"}]}]}"#;
    let Ok(c) = load_text("count", text) else {
        unreachable!("well-formed")
    };
    let Ok((report, header)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(report.measured());
    assert_eq!(report.exit_code(), 1);
    assert_eq!((report.channels, report.actions, report.count()), (2, 4, 1));
    assert_eq!(report.accepting_more[0].0, "more");
    assert_eq!(report.accepting_more[0].1, ["x", "y"]);
    let s = report.summary(&header);
    assert!(s.contains("channels_accepting_more_than_pty=1"), "{s}");
    assert!(s.contains("more accepts more"), "{s}");
}

#[test]
fn a_catalogue_that_does_not_fit_is_refused_with_the_reason() {
    let cases = [
        (r#"{"channels":[]}"#, "no revision"),
        (r#"{"revision":"t"}"#, "no channels list"),
        (
            r#"{"revision":"t","channels":[{"id":"a","actions":[]},{"id":"a","actions":[]}]}"#,
            "duplicate channel id a",
        ),
        (
            r#"{"revision":"t","channels":[{"id":"a","actions":[{"id":"x"},{"id":"x"}]}]}"#,
            "duplicate action id x",
        ),
        (
            r#"{"revision":"t","channels":[{"id":"a","actions":[{"id":"x","pty_equivalent":""}]}]}"#,
            "pty_equivalent must be a sentence or null",
        ),
        (
            r#"{"revision":"t","channels":[{"id":"a","actions":[{"pty_equivalent":null}]}]}"#,
            "an action needs an id",
        ),
    ];
    for (i, (text, want)) in cases.iter().enumerate() {
        let e = load_text(&format!("bad{i}"), text)
            .err()
            .unwrap_or_default();
        assert!(e.contains(want), "{want}: {e}");
    }
}

#[test]
fn only_a_measured_zero_exits_zero() {
    let text = r#"{"revision":"t","channels":[
        {"id":"ok","actions":[{"id":"a","pty_equivalent":"write the same bytes"}]}]}"#;
    let Ok(c) = load_text("zero", text) else {
        unreachable!("well-formed")
    };
    let Ok((report, _)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(report.measured());
    assert_eq!(report.exit_code(), 0);
}
