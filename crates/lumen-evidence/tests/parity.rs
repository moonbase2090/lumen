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
fn the_committed_catalogue_declares_the_spec_channels_and_says_they_have_no_code() {
    let Ok(c) = load_catalogue(&committed()) else {
        unreachable!("the committed catalogue loads")
    };
    assert_eq!(c.revision, "ch-2");
    let ids: Vec<_> = c.channels.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["track-b", "seal-channel", "control-channel"]);
    for ch in &c.channels {
        assert!(ch.source.is_some(), "{}: names its spec section", ch.id);
        assert!(!ch.actions.is_empty(), "{}", ch.id);
        // A channel that names code must name code that exists.
        if let Some(code) = &ch.code {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(code);
            assert!(path.exists(), "{}: {code}", ch.id);
        }
    }
    let Ok((report, header)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(report.measured());
    assert_eq!((report.channels, report.count()), (3, 0));
    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.without_code.len(), 3);
    let s = report.summary(&header);
    assert!(s.contains("channels_accepting_more_than_pty=0"), "{s}");
    assert!(
        s.contains("declaration_level: 3 of 3 channels have no code"),
        "{s}"
    );
}

#[test]
fn an_empty_catalogue_is_still_not_measured_and_exits_3() {
    let Ok(c) = load_text("empty", r#"{"revision":"t","channels":[]}"#) else {
        unreachable!("well-formed")
    };
    let Ok((report, header)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert!(!report.measured());
    assert_eq!(report.exit_code(), 3, "an empty catalogue is not a pass");
    assert!(report.summary(&header).contains("2.9=not measured"));
}

#[test]
fn a_channel_with_code_is_not_called_declaration_level() {
    let text = r#"{"revision":"t","channels":[
        {"id":"real","code":"crates/lumen-sealed","actions":[{"id":"a","pty_equivalent":"the same bytes"}]}]}"#;
    let Ok(c) = load_text("code", text) else {
        unreachable!("well-formed")
    };
    let Ok((report, header)) = run_parity(&c, "testsha", Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.without_code, Vec::<String>::new());
    assert!(!report.summary(&header).contains("declaration_level"));
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
