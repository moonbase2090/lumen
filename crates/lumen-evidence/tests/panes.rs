//! Loading a pane-corpus directory.

use lumen_evidence::panes::load_panes;
use serde_json::json;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    assert!(std::fs::create_dir_all(&d).is_ok());
    d
}

fn file(revision: &str, ids: &[&str]) -> String {
    let cases: Vec<_> = ids
        .iter()
        .map(|id| json!({"id": id, "category": "t", "screen": {"cols": 80, "rows": 24}, "script": []}))
        .collect();
    json!({"revision": revision, "cases": cases}).to_string()
}

#[test]
fn an_empty_directory_is_an_error() {
    assert!(load_panes(&dir("panes-empty")).is_err());
}

#[test]
fn files_load_in_name_order_under_one_revision() {
    let d = dir("panes-order");
    assert!(std::fs::write(d.join("b.json"), file("pc-1", &["second"])).is_ok());
    assert!(std::fs::write(d.join("a.json"), file("pc-1", &["first"])).is_ok());
    let Ok(c) = load_panes(&d) else {
        unreachable!("two files load")
    };
    assert_eq!(c.revision, "pc-1");
    let ids: Vec<_> = c.cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["first", "second"]);
}

#[test]
fn a_repeated_id_across_files_is_refused() {
    let d = dir("panes-dup");
    assert!(std::fs::write(d.join("a.json"), file("pc-1", &["same"])).is_ok());
    assert!(std::fs::write(d.join("b.json"), file("pc-1", &["same"])).is_ok());
    let Err(e) = load_panes(&d) else {
        unreachable!("refused")
    };
    assert!(e.contains("used twice"), "{e}");
}

#[test]
fn files_that_disagree_on_the_revision_are_refused() {
    let d = dir("panes-rev");
    assert!(std::fs::write(d.join("a.json"), file("pc-1", &["x"])).is_ok());
    assert!(std::fs::write(d.join("b.json"), file("pc-2", &["y"])).is_ok());
    let Err(e) = load_panes(&d) else {
        unreachable!("refused")
    };
    assert!(e.contains("differs"), "{e}");
}
