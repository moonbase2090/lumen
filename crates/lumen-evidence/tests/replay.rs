//! Replays the fixture corpus through the driver.

use lumen_evidence::{load_corpus, run};
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
    assert_eq!(header.sinks, "none");
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
