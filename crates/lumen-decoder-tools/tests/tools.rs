//! The classifier is checked against excerpts of real libFuzzer and cargo logs
//! (in `fixtures`), the seeds against the real decoder, and the ledger against
//! the real judge in `lumen-evidence`.

#![allow(missing_docs)]

use std::error::Error;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::Format;
use lumen_decoder::unlock::{GateRecord, Identities, images_enabled};
use lumen_decoder_tools::classify::{Kind, classify, classify_replay};
use lumen_decoder_tools::identity::Kind as Identity;
use lumen_decoder_tools::{escape, identity, ledger};

#[path = "../examples/seeds/generate.rs"]
mod seeds;
use lumen_evidence::decoder::{Report, judge};
use serde_json::json;

type R = Result<(), Box<dyn Error>>;

fn fixture(name: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))?)
}

#[test]
fn each_kind_of_crash_is_told_apart() -> R {
    assert_eq!(
        classify(&fixture("libfuzzer-timeout.log")?),
        Some(Kind::Hang)
    );
    assert_eq!(
        classify(&fixture("libfuzzer-rss-limit.log")?),
        Some(Kind::Memory)
    );
    assert_eq!(
        classify(&fixture("libfuzzer-alloc-abort.log")?),
        Some(Kind::Memory)
    );
    assert_eq!(
        classify(&fixture("libfuzzer-asan.log")?),
        Some(Kind::Sanitizer)
    );
    assert_eq!(
        classify(&fixture("libfuzzer-panic.log")?),
        Some(Kind::Panic)
    );
    Ok(())
}

#[test]
fn a_clean_run_is_not_a_crash() -> R {
    assert_eq!(classify(&fixture("libfuzzer-clean.log")?), None);
    Ok(())
}

#[test]
fn a_sanitizer_report_wins_over_the_fuzzers_deadly_signal_line() {
    let log = "==1==ERROR: AddressSanitizer: heap-use-after-free\n==1== ERROR: libFuzzer: deadly signal\n";
    assert_eq!(classify(log), Some(Kind::Sanitizer));
    assert_eq!(
        classify("==1== ERROR: libFuzzer: deadly signal\n"),
        Some(Kind::Other)
    );
}

#[test]
fn the_escape_counts_come_from_the_g4_binary_only() -> R {
    let log = fixture("cargo-test-g4.txt")?;
    assert_eq!(escape::counts(&log, "g4_sandbox"), Some((33, 33)));
    assert_eq!(escape::counts(&log, "other_binary"), None);
    let two = "Running a (target/debug/deps/a-1)\ntest result: ok. 5 passed; 0 failed; 0 ignored;\n\
               Running tests/g4_sandbox.rs (target/debug/deps/g4_sandbox-2)\ntest result: FAILED. 30 passed; 2 failed; 1 ignored;\n";
    assert_eq!(escape::counts(two, "g4_sandbox"), Some((30, 33)));
    Ok(())
}

#[test]
fn every_seed_decodes_with_the_real_decoder() -> R {
    let all = seeds::all()?;
    assert!(all.len() >= 14);
    for (name, bytes) in &all {
        let format = match name.split('/').next() {
            Some("png") => Format::Png,
            Some("jpeg") => Format::Jpeg,
            _ => Format::Webp,
        };
        let img = decode(format, bytes).map_err(|s| format!("{name}: {s:?}"))?;
        assert_eq!(
            img.rgba.len(),
            img.width as usize * img.height as usize * 4,
            "{name}"
        );
    }
    for dir in ["png/", "jpeg/", "webp/"] {
        assert!(all.iter().any(|(n, _)| n.starts_with(dir)), "{dir}");
    }
    Ok(())
}

fn tree(files: &[(&str, &str)]) -> Result<std::path::PathBuf, Box<dyn Error>> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "lumen-id-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    for (path, body) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().ok_or("no parent")?)?;
        std::fs::write(p, body)?;
    }
    Ok(root)
}

const BASE: [(&str, &str); 15] = [
    ("crates/lumen-decoder/src/decode.rs", "d"),
    ("crates/lumen-decoder/src/ffi_sandbox.rs", "s"),
    ("crates/lumen-decoder/src/ffi_alloc.rs", "a"),
    ("crates/lumen-decoder/src/limits.rs", "l"),
    ("crates/lumen-decoder/src/protocol.rs", "p"),
    ("crates/lumen-decoder/src/bin/lumen-decoder-worker.rs", "w"),
    ("crates/lumen-decoder/src/runner.rs", "r"),
    ("crates/lumen-decoder/src/ffi_process.rs", "f"),
    ("crates/lumen-decoder/src/unlock.rs", "u"),
    ("crates/lumen-decoder/src/images.rs", "g"),
    ("crates/lumen-decoder/src/lib.rs", "i"),
    ("crates/lumen-decoder/src/bin/lumen-decoder-hostile.rs", "h"),
    ("crates/lumen-decoder/Cargo.toml", "m"),
    ("Cargo.lock", "k"),
    ("crates/lumen-decoder/src/notes.txt", "n"),
];

fn id(
    root: &std::path::Path,
    kind: Identity,
    compiler: &str,
    flags: &str,
) -> Result<String, Box<dyn Error>> {
    Ok(identity::hash(
        root,
        &identity::files(root, kind)?,
        compiler,
        flags,
    )?)
}

#[test]
fn each_identity_changes_with_the_inputs_that_belong_to_it() -> R {
    let root = tree(&BASE)?;
    for kind in [Identity::Decoder, Identity::ImagePath] {
        let base = id(&root, kind, "rustc 1", "")?;
        assert_eq!(base, id(&root, kind, "rustc 1", "")?, "{kind:?} stable");
        assert_ne!(base, id(&root, kind, "rustc 2", "")?, "{kind:?} compiler");
        assert_ne!(
            base,
            id(&root, kind, "rustc 1", "-Cdebuginfo=2")?,
            "{kind:?} flags"
        );
        for path in [
            "crates/lumen-decoder/Cargo.toml",
            "Cargo.lock",
            "crates/lumen-decoder/src/limits.rs",
            "crates/lumen-decoder/src/protocol.rs",
        ] {
            let before = std::fs::read_to_string(root.join(path))?;
            std::fs::write(root.join(path), "changed")?;
            assert_ne!(base, id(&root, kind, "rustc 1", "")?, "{kind:?}: {path}");
            std::fs::write(root.join(path), before)?;
        }
        assert_eq!(base, id(&root, kind, "rustc 1", "")?, "{kind:?} restored");
    }
    Ok(())
}

#[test]
fn a_change_on_one_side_moves_only_that_identity() -> R {
    let root = tree(&BASE)?;
    let (d, p) = (
        id(&root, Identity::Decoder, "c", "")?,
        id(&root, Identity::ImagePath, "c", "")?,
    );
    assert_ne!(d, p);
    // Worker-only source: the decoder identity moves, the image path's does not.
    std::fs::write(root.join("crates/lumen-decoder/src/decode.rs"), "changed")?;
    assert_ne!(d, id(&root, Identity::Decoder, "c", "")?);
    assert_eq!(p, id(&root, Identity::ImagePath, "c", "")?);
    // Host-only source: the other way round.
    let d2 = id(&root, Identity::Decoder, "c", "")?;
    std::fs::write(root.join("crates/lumen-decoder/src/runner.rs"), "changed")?;
    assert_eq!(d2, id(&root, Identity::Decoder, "c", "")?);
    assert_ne!(p, id(&root, Identity::ImagePath, "c", "")?);
    Ok(())
}

#[test]
fn the_test_only_worker_is_in_neither_identity() -> R {
    let root = tree(&BASE)?;
    let before = [
        id(&root, Identity::Decoder, "c", "")?,
        id(&root, Identity::ImagePath, "c", "")?,
    ];
    std::fs::write(
        root.join("crates/lumen-decoder/src/bin/lumen-decoder-hostile.rs"),
        "different",
    )?;
    assert_eq!(
        before,
        [
            id(&root, Identity::Decoder, "c", "")?,
            id(&root, Identity::ImagePath, "c", "")?
        ]
    );
    Ok(())
}

#[test]
fn a_listed_source_that_is_missing_is_an_error_not_a_smaller_identity() -> R {
    let root = tree(&BASE)?;
    std::fs::remove_file(root.join("crates/lumen-decoder/src/runner.rs"))?;
    assert!(identity::files(&root, Identity::ImagePath).is_err());
    assert!(
        identity::files(&root, Identity::Decoder).is_ok(),
        "the decoder does not list it"
    );
    Ok(())
}

#[test]
fn a_source_file_in_neither_list_is_reported() -> R {
    let root = tree(&BASE)?;
    let extra = identity::uncovered(&root)?;
    assert_eq!(extra.len(), 1, "{extra:?}");
    assert!(extra[0].ends_with("notes.txt"));
    Ok(())
}

#[test]
fn every_source_file_of_the_real_crate_is_in_an_identity() -> R {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let extra = identity::uncovered(&root)?;
    assert!(
        extra.is_empty(),
        "add these to DECODER_SOURCES or IMAGE_PATH_SOURCES: {extra:?}"
    );
    for kind in [Identity::Decoder, Identity::ImagePath] {
        identity::files(&root, kind)?;
    }
    Ok(())
}

fn entry(hours: f64, crash_at: Option<f64>) -> Result<serde_json::Value, String> {
    let samples = vec![
        json!({"at_cpu_hour": 100.0, "edges": 900}),
        json!({"at_cpu_hour": hours - 300.0, "edges": 950}),
        json!({"at_cpu_hour": hours, "edges": 950}),
    ];
    let crashes: Vec<_> = crash_at
        .map(|h| json!({"at_cpu_hour": h, "kind": "hang"}))
        .into_iter()
        .collect();
    ledger::format_entry("png", &samples, &crashes)
}

fn judged(entry: serde_json::Value, passed: u32) -> Result<Report, String> {
    let l = ledger::ledger("fz-1", "d", "p", &[entry], "se", passed, 33);
    judge(&l.to_string())
}

#[test]
fn a_ledger_from_the_tools_is_judged_by_the_real_gate() -> R {
    assert!(matches!(
        judged(entry(1200.0, None)?, 33)?,
        Report::Judged { .. }
    ));
    let pass = judged(entry(1200.0, None)?, 33)?;
    assert!(pass.passes(), "{pass:?}");
    assert!(
        judged(entry(500.0, None)?, 33)?.passes(),
        "above the 250 floor, not the earlier 1000"
    );
    assert!(
        !judged(entry(249.0, None)?, 33)?.passes(),
        "too few CPU-hours"
    );
    assert!(
        !judged(entry(1200.0, Some(1100.0))?, 33)?.passes(),
        "a recent crash"
    );
    assert!(
        !judged(entry(1200.0, Some(40.0))?, 33)?.passes(),
        "an old crash still fails the build identity"
    );
    assert!(
        !judged(entry(1200.0, None)?, 32)?.passes(),
        "an escape test failed"
    );
    Ok(())
}

#[test]
fn a_format_with_no_samples_has_no_cpu_hours_and_cannot_pass() -> R {
    let e = ledger::format_entry("webp", &[], &[])?;
    assert_eq!(e["cpu_hours"], json!(0.0));
    assert!(!judged(e, 33)?.passes());
    Ok(())
}

#[test]
fn an_oom_report_that_replays_small_is_an_asan_artifact_not_a_crash() -> R {
    // A record written by scripts/decoder-replay.py for the real input behind
    // the 676 MB report: the real worker peaked at about 55 MiB.
    let real: serde_json::Value = serde_json::from_str(&fixture("replay-oom-jpeg.json")?)?;
    assert_eq!(classify_replay(&real), Some(Kind::AsanArtifact));
    Ok(())
}

#[test]
fn a_replay_over_the_limit_or_aborted_by_the_cap_is_a_memory_crash() {
    // Hand-written records: no real input is known to need this much.
    let over = json!({"peak_rss_kb": 512 * 1024 + 1, "exit": 0});
    let at = json!({"peak_rss_kb": 512 * 1024, "exit": 0});
    let aborted = json!({"peak_rss_kb": 300_000, "exit": -6});
    assert_eq!(classify_replay(&over), Some(Kind::Memory));
    assert_eq!(classify_replay(&at), Some(Kind::AsanArtifact));
    assert_eq!(classify_replay(&aborted), Some(Kind::Memory));
    assert_eq!(classify_replay(&json!({"exit": 0})), None);
}

#[test]
fn notes_travel_with_the_ledger_and_do_not_change_the_verdict() -> R {
    let base = ledger::ledger("fz-1", "d", "p", &[entry(1200.0, None)?], "se", 33, 33);
    let notes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!(
        "{}/../lumen-decoder/fuzz/seeds-regression/PAST_CVES.json",
        env!("CARGO_MANIFEST_DIR")
    ))?)?;
    assert_eq!(notes["result"], "empty");
    let annotated = ledger::with_notes(base.clone(), json!({"past_cve_seeds": notes}));
    assert_eq!(annotated["notes"]["past_cve_seeds"]["result"], "empty");
    let (a, b) = (judge(&base.to_string())?, judge(&annotated.to_string())?);
    assert_eq!(a, b);
    assert!(b.passes());
    Ok(())
}

// The unlock rule against identities computed from a real file tree: each way a
// build can change moves an identity, and the alt text is shown until the gate
// has passed for the new pair.

fn pair(root: &std::path::Path, compiler: &str, flags: &str) -> Result<Identities, Box<dyn Error>> {
    Ok(Identities {
        decoder_build: id(root, Identity::Decoder, compiler, flags)?,
        image_path_build: id(root, Identity::ImagePath, compiler, flags)?,
    })
}

fn passed(ids: &Identities) -> GateRecord {
    GateRecord {
        platform: "linux".into(),
        decoder_build: ids.decoder_build.clone(),
        image_path_build: ids.image_path_build.clone(),
        decoder_gate_passed: true,
        memory_gate_passed: true,
    }
}

#[test]
fn images_stay_on_for_the_build_the_gate_passed_for() -> R {
    let root = tree(&BASE)?;
    let ids = pair(&root, "rustc 1", "")?;
    assert!(images_enabled(true, "linux", &ids, &[passed(&ids)]));
    Ok(())
}

#[test]
fn a_changed_source_file_shows_alt_text() -> R {
    for path in [
        "crates/lumen-decoder/src/decode.rs",
        "crates/lumen-decoder/src/runner.rs",
    ] {
        let root = tree(&BASE)?;
        let record = passed(&pair(&root, "rustc 1", "")?);
        std::fs::write(root.join(path), "changed")?;
        let now = pair(&root, "rustc 1", "")?;
        assert!(!images_enabled(true, "linux", &now, &[record]), "{path}");
    }
    Ok(())
}

#[test]
fn a_changed_compiler_flag_shows_alt_text() -> R {
    let root = tree(&BASE)?;
    let record = passed(&pair(&root, "rustc 1", "")?);
    let now = pair(&root, "rustc 1", "-Cdebuginfo=2")?;
    assert!(!images_enabled(true, "linux", &now, &[record]));
    Ok(())
}

#[test]
fn a_changed_compiler_shows_alt_text() -> R {
    let root = tree(&BASE)?;
    let record = passed(&pair(&root, "rustc 1", "")?);
    let now = pair(&root, "rustc 2", "")?;
    assert!(!images_enabled(true, "linux", &now, &[record]));
    Ok(())
}

#[test]
fn a_dependency_bump_in_the_lockfile_shows_alt_text() -> R {
    let root = tree(&BASE)?;
    let record = passed(&pair(&root, "rustc 1", "")?);
    std::fs::write(
        root.join("Cargo.lock"),
        "name = \"png\"\nversion = \"0.18.2\"\n",
    )?;
    let now = pair(&root, "rustc 1", "")?;
    assert!(!images_enabled(true, "linux", &now, &[record]));
    Ok(())
}

#[test]
fn a_gate_that_passed_for_only_one_identity_shows_alt_text() -> R {
    // The decoder is as the gate saw it; the image path changed since.
    let root = tree(&BASE)?;
    let record = passed(&pair(&root, "rustc 1", "")?);
    std::fs::write(root.join("crates/lumen-decoder/src/runner.rs"), "changed")?;
    let now = pair(&root, "rustc 1", "")?;
    assert_eq!(now.decoder_build, record.decoder_build);
    assert_ne!(now.image_path_build, record.image_path_build);
    assert!(!images_enabled(true, "linux", &now, &[record]));
    Ok(())
}
