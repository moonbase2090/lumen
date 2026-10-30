//! The Miri record checks. The committed record is the one the script wrote on
//! a real run; these tests also show the checks reject a record that says
//! something its own lines do not.

#![allow(missing_docs)]

use lumen_decoder_tools::miri::{same_results, validate};
use serde_json::{Value, json};

fn record() -> Value {
    json!({
        "toolchain": "rustc 1.99.0-nightly", "miri": "miri 0.1.0", "host": "Darwin 27.0.0 arm64",
        "date": "2026-10-29", "repo_commit": "a".repeat(40), "upstream_commit": "d22e2aa",
        "runs": [
            {"name": "unit", "status": "ok", "command": "cargo miri test",
             "results": ["test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"],
             "first_error": null},
            {"name": "neon", "status": "FAILED", "command": "cargo miri test",
             "results": [], "first_error": "error: unsupported operation: can't call LLVM intrinsic"}
        ]
    })
}

#[test]
fn a_consistent_record_is_valid() {
    assert_eq!(validate(&record()), Ok(()));
}

#[test]
fn a_passed_run_whose_lines_show_a_failure_is_rejected() {
    let mut r = record();
    r["runs"][0]["results"] = json!(["test result: FAILED. 9 passed; 1 failed; 0 ignored"]);
    assert!(validate(&r).is_err());
    r["runs"][0]["results"] = json!(["test result: ok. 9 passed; 1 failed; 0 ignored"]);
    assert!(validate(&r).is_err(), "ok with a failed count");
}

#[test]
fn a_passed_run_with_no_result_line_is_rejected() {
    let mut r = record();
    r["runs"][0]["results"] = json!([]);
    assert!(validate(&r).is_err());
}

#[test]
fn a_failed_run_must_say_why() {
    let mut r = record();
    r["runs"][1]["first_error"] = Value::Null;
    assert!(validate(&r).is_err());
}

#[test]
fn a_record_without_its_provenance_is_rejected() {
    for key in [
        "toolchain",
        "miri",
        "host",
        "date",
        "repo_commit",
        "upstream_commit",
    ] {
        let mut r = record();
        r[key] = json!("");
        assert!(validate(&r).is_err(), "{key}");
    }
    let mut r = record();
    r["runs"] = json!([]);
    assert!(validate(&r).is_err());
}

#[test]
fn a_fresh_record_matches_when_only_provenance_differs() {
    let mut fresh = record();
    fresh["host"] = json!("Linux 7.0 aarch64");
    fresh["date"] = json!("2026-11-02");
    fresh["repo_commit"] = json!("b".repeat(40));
    assert_eq!(same_results(&fresh, &record()), Ok(()));
}

#[test]
fn a_fresh_record_that_differs_in_a_result_is_not_the_committed_one() {
    let committed = record();
    for (path, value) in [
        ("status", json!("FAILED")),
        (
            "results",
            json!(["test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"]),
        ),
        ("name", json!("renamed")),
    ] {
        let mut fresh = record();
        fresh["runs"][0][path] = value;
        if path == "status" {
            fresh["runs"][0]["first_error"] = json!("x");
        }
        assert!(same_results(&fresh, &committed).is_err(), "{path}");
    }
    let mut fresh = record();
    fresh["runs"][1]["first_error"] = json!("error: unsupported operation: a different intrinsic");
    assert!(same_results(&fresh, &committed).is_err(), "error line");
    let mut fresh = record();
    fresh["upstream_commit"] = json!("other");
    assert!(same_results(&fresh, &committed).is_err(), "upstream commit");
    let mut fresh = record();
    fresh["runs"].as_array_mut().map(Vec::pop);
    assert!(same_results(&fresh, &committed).is_err(), "run count");
}

#[test]
fn the_committed_record_is_valid_and_says_what_the_docs_say()
-> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(format!(
        "{}/../lumen-decoder/fuzz/miri-record.json",
        env!("CARGO_MANIFEST_DIR")
    ))?;
    let r: Value = serde_json::from_str(&text)?;
    validate(&r)?;
    let run = |name: &str| {
        r["runs"]
            .as_array()
            .and_then(|v| v.iter().find(|x| x["name"] == name))
            .cloned()
    };
    // Passed on the AVX2 code.
    for name in [
        "upstream-x86-avx2-unit",
        "upstream-x86-default",
        "upstream-x86-avx2-integ-arithmetic_after_eoi",
        "upstream-x86-avx2-integ-arithmetic_progressive",
        "upstream-x86-avx2-integ-invalid_images",
        "upstream-x86-avx2-integ-metadata",
        "upstream-x86-avx2-integ-planar_layout",
        "upstream-x86-avx2-integ-progressive_dc_tables",
        "seeds-x86-avx2",
    ] {
        assert_eq!(run(name).ok_or(name)?["status"], "ok", "{name}");
    }
    assert_eq!(r["zune_jpeg_version"], "0.5.16-rc3");
    // The runs that did not finish are recorded as such, with the reason, not dropped.
    let timeouts = r["runs"]
        .as_array()
        .map(|v| v.iter().filter(|x| x["status"] == "TIMEOUT").count());
    assert_eq!(timeouts, Some(9));
    // The one test that compares function addresses fails on its assertion.
    let fnptr = run("upstream-x86-fnptr-test").ok_or("fnptr run")?;
    assert_eq!(fnptr["status"], "FAILED");
    assert!(
        fnptr["first_error"]
            .as_str()
            .unwrap_or("")
            .contains("assertion")
    );
    // The NEON run is recorded as a Miri limitation, not hidden.
    let neon = run("seeds-host-neon").ok_or("neon run")?;
    assert_eq!(neon["status"], "FAILED");
    assert!(
        neon["first_error"]
            .as_str()
            .unwrap_or("")
            .contains("unsupported operation")
    );
    // The docs' counts: 20 unit tests, and 9 + 11 + 8 + 3 in the integration files that ran tests.
    let passed = |name: &str| -> Option<String> {
        run(name)?["results"][0]
            .as_str()
            .and_then(|l| l.split_whitespace().nth(3).map(str::to_string))
    };
    assert_eq!(passed("upstream-x86-avx2-unit").as_deref(), Some("20"));
    for (name, n) in [
        ("invalid_images", "9"),
        ("metadata", "11"),
        ("planar_layout", "8"),
        ("progressive_dc_tables", "3"),
    ] {
        assert_eq!(
            passed(&format!("upstream-x86-avx2-integ-{name}")).as_deref(),
            Some(n),
            "{name}"
        );
    }
    Ok(())
}
