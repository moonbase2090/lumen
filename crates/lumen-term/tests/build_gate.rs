//! The build script's output: every call sets all three values, and only a
//! record for a supported target unlocks.

#![allow(missing_docs)]

#[path = "../build_support/gate.rs"]
mod gate;

const RECORDS: &str =
    "# comment\nlinux d1 p1 pass pass\nmacos d1 p1 pass pass\nwindows d1 p1 pass pass\n";

fn value<'a>(lines: &'a [String], name: &str) -> &'a str {
    let key = format!("cargo:rustc-env={name}=");
    lines
        .iter()
        .find_map(|l| l.strip_prefix(&key))
        .unwrap_or("<not set>")
}

fn assert_locked(lines: &[String]) {
    assert_eq!(lines.len(), 3);
    assert_eq!(value(lines, "LUMEN_DECODER_BUILD"), gate::LOCKED);
    assert_eq!(value(lines, "LUMEN_IMAGE_PATH_BUILD"), gate::LOCKED);
    assert_eq!(value(lines, "LUMEN_GATE_RECORDS"), "");
}

#[test]
fn a_missing_records_file_sets_all_three_values_to_locked() {
    // Setting them is what stops an inherited value from reaching the compiler.
    assert_locked(&gate::directives(None, "linux"));
}

#[test]
fn an_unsupported_target_is_locked_even_with_records() {
    for os in ["freebsd", "android", "ios", "", "Linux"] {
        assert_locked(&gate::directives(Some(RECORDS), os));
    }
}

#[test]
fn a_supported_target_without_a_record_is_locked() {
    assert_locked(&gate::directives(Some("macos d p pass pass\n"), "linux"));
    assert_locked(&gate::directives(Some("# only a comment\n"), "linux"));
    assert_locked(&gate::directives(Some("linux d p pass\n"), "linux"));
}

#[test]
fn each_supported_target_gets_its_own_record_and_the_records() {
    for os in ["linux", "macos", "windows"] {
        let lines = gate::directives(Some(RECORDS), os);
        assert_eq!(lines.len(), 3);
        assert_eq!(value(&lines, "LUMEN_DECODER_BUILD"), "d1");
        assert_eq!(value(&lines, "LUMEN_IMAGE_PATH_BUILD"), "p1");
        assert_eq!(
            value(&lines, "LUMEN_GATE_RECORDS"),
            "linux d1 p1 pass pass;macos d1 p1 pass pass;windows d1 p1 pass pass"
        );
    }
}
