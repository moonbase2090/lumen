//! Reach mode: the committed session matrix and the checks on its shape.

use lumen_evidence::matrix::{ReachArm, SESSION_TYPES, load_matrix, run_reach};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/session-matrix.json")
}

fn load_doc() -> Value {
    let text = std::fs::read_to_string(committed()).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// Write `doc` to a temp file and try to load it as a matrix.
fn load_edited(name: &str, doc: &Value) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("lumen-reach-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("m.json");
    assert!(std::fs::write(&path, doc.to_string()).is_ok());
    load_matrix(&path).map(|_| ())
}

#[test]
fn the_committed_matrix_has_one_session_per_type_per_platform() {
    let Ok(m) = load_matrix(&committed()) else {
        unreachable!("the committed matrix loads")
    };
    assert_eq!(m.sessions.len(), 14);
    for t in SESSION_TYPES {
        for p in ["linux", "macos"] {
            let n = m
                .sessions
                .iter()
                .filter(|s| s.session == t && s.platform == p)
                .count();
            assert_eq!(n, 1, "{t} on {p}");
        }
    }
}

#[test]
fn both_arms_are_replayed_and_the_log_header_names_the_harness_and_matrix() {
    let Ok(m) = load_matrix(&committed()) else {
        unreachable!("the committed matrix loads")
    };
    for arm in [ReachArm::B, ReachArm::A] {
        let mut log = Vec::new();
        let Ok((report, header)) = run_reach(&m, "testsha", arm, &mut log) else {
            unreachable!("writing to a Vec succeeds")
        };
        assert_eq!(report.sessions.len(), 14);
        assert_eq!(report.reached(), 14, "{}", report.summary(&header));
        assert_eq!(header.session_matrix, m.revision);
        let text = String::from_utf8(log).unwrap_or_default();
        let first = text.lines().next().unwrap_or_default();
        assert!(first.contains("\"harness\":\"lumen-evidence-"), "{first}");
        assert!(first.contains("\"session_matrix\":\"sm-1\""), "{first}");
        assert!(first.contains("\"corpus\":\"none\""), "{first}");
        let markers = text.lines().filter(|l| l.starts_with("{\"case\"")).count();
        assert_eq!(markers, 14);
    }
}

#[test]
fn a_session_that_shows_something_else_is_a_miss() {
    let mut doc = load_doc();
    doc["sessions"][0]["expect"]["b"] = serde_json::json!(["output", "text"]);
    let dir = std::env::temp_dir().join(format!("lumen-reach-miss-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("m.json");
    assert!(std::fs::write(&path, doc.to_string()).is_ok());
    let Ok(m) = load_matrix(&path) else {
        unreachable!("the edited matrix still loads")
    };
    let Ok((report, header)) = run_reach(&m, "testsha", ReachArm::B, Vec::new()) else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.reached(), 13);
    let summary = report.summary(&header);
    assert!(summary.contains("2.1 reach=13/14 (meets"), "{summary}");
    assert!(summary.contains("linux-local-shell local-shell linux missed got=[\"output\", \"block\"] want=[\"output\", \"text\"]"));
}

#[test]
fn a_matrix_that_does_not_fit_is_refused_with_the_reason() {
    let base = load_doc();

    let mut short = base.clone();
    if let Some(a) = short["sessions"].as_array_mut() {
        a.pop();
    }
    let e = load_edited("short", &short).err().unwrap_or_default();
    assert!(e.contains("13 sessions"), "{e}");

    let mut dup = base.clone();
    dup["sessions"][1] = dup["sessions"][0].clone();
    let e = load_edited("dup", &dup).err().unwrap_or_default();
    assert!(e.contains("duplicate session id"), "{e}");

    let mut cell = base.clone();
    cell["sessions"][1]["session"] = cell["sessions"][0]["session"].clone();
    let e = load_edited("cell", &cell).err().unwrap_or_default();
    assert!(e.contains("a second local-shell session on linux"), "{e}");

    let mut no_expect = base.clone();
    no_expect["sessions"][0]["expect"] = serde_json::json!({"b": ["output"]});
    let e = load_edited("noexp", &no_expect).err().unwrap_or_default();
    assert!(e.contains("expect needs a list for \"a\""), "{e}");

    let mut typo = base.clone();
    typo["sessions"][0]["expect"]["b"] = serde_json::json!(["output", "blok"]);
    let e = load_edited("typo", &typo).err().unwrap_or_default();
    assert!(e.contains("unknown expect kind \"blok\""), "{e}");

    let mut key = base.clone();
    key["sessions"][0]["expect"]["c"] = serde_json::json!([]);
    let e = load_edited("key", &key).err().unwrap_or_default();
    assert!(e.contains("unknown expect key \"c\""), "{e}");

    let mut ty = base;
    ty["sessions"][0]["session"] = serde_json::json!("telnet");
    let e = load_edited("ty", &ty).err().unwrap_or_default();
    assert!(e.contains("session must be one of"), "{e}");
}
