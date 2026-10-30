//! The JPEG fixtures that stand behind claims in the docs and the PR text are
//! identified by hash, and the timing claim is a committed measurement, so both
//! can be checked by someone else:
//!
//! - `jpeg-panic-*.jpg`: inputs that panic `zune-jpeg` 0.5.15.
//! - `jpeg-slow-*.jpg`: inputs that ran past the fuzzer's 2 s timeout in the
//!   instrumented build and which the release worker answers in well under it.
//!   `jpeg-slow-timing.json` was written by `scripts/decoder-replay.py` running
//!   each through the release worker; the test checks it covers exactly these
//!   files, and that nothing in it is near the worker's limits.

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::Format;
use serde_json::Value;
use sha2::{Digest, Sha256};

type R = Result<(), Box<dyn Error>>;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lumen-decoder/tests/fixtures")
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn manifest(name: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let text = std::fs::read_to_string(dir().join(name))?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_once("  "))
        .map(|(hash, file)| (file.to_string(), hash.to_string()))
        .collect())
}

fn files(prefix: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut v: Vec<String> = std::fs::read_dir(dir())?
        .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
        .filter(|n| {
            n.starts_with(prefix)
                && std::path::Path::new(n)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("jpg"))
        })
        .collect();
    v.sort();
    Ok(v)
}

fn check_manifest(prefix: &str, manifest_name: &str, count: usize) -> R {
    let listed = manifest(manifest_name)?;
    let on_disk = files(prefix)?;
    assert_eq!(on_disk.len(), count, "{prefix}: files on disk");
    assert_eq!(
        listed.keys().cloned().collect::<Vec<_>>(),
        on_disk,
        "{prefix}: the manifest lists exactly the files"
    );
    for (file, hash) in &listed {
        assert_eq!(&hex(&std::fs::read(dir().join(file))?), hash, "{file}");
    }
    Ok(())
}

#[test]
fn the_panic_fixtures_are_the_files_the_manifest_names() -> R {
    check_manifest("jpeg-panic-", "jpeg-panic.sha256", 16)
}

#[test]
fn the_slow_fixtures_are_the_files_the_manifest_names() -> R {
    check_manifest("jpeg-slow-", "jpeg-slow.sha256", 9)
}

#[test]
fn the_timing_record_covers_exactly_the_slow_fixtures_and_nothing_is_near_the_limits() -> R {
    let rec: Value = serde_json::from_str(&std::fs::read_to_string(
        dir().join("jpeg-slow-timing.json"),
    )?)?;
    for key in ["host", "machine", "date", "command", "worker"] {
        assert!(rec[key].as_str().is_some_and(|s| !s.is_empty()), "{key}");
    }
    let results = rec["results"].as_array().ok_or("no results")?;
    let mut named: Vec<String> = results
        .iter()
        .filter_map(|r| r["file"].as_str().map(str::to_string))
        .collect();
    named.sort();
    assert_eq!(named, files("jpeg-slow-")?);
    for r in results {
        let file = r["file"].as_str().unwrap_or("?");
        assert_eq!(r["exit"], 0, "{file}: the worker exited normally");
        assert!(
            matches!(r["status"].as_u64(), Some(0 | 4)),
            "{file}: decoded or rejected, got {}",
            r["status"]
        );
        // Far from the 2 s CPU limit and the 512 MiB memory limit.
        assert!(
            r["cpu_seconds"].as_f64().is_some_and(|s| s < 1.0),
            "{file}: cpu {}",
            r["cpu_seconds"]
        );
        assert!(
            r["wall_seconds"].as_f64().is_some_and(|s| s < 1.0),
            "{file}: wall {}",
            r["wall_seconds"]
        );
        assert!(
            r["peak_rss_kb"].as_u64().is_some_and(|k| k < 256 * 1024),
            "{file}: peak {}",
            r["peak_rss_kb"]
        );
    }
    Ok(())
}

#[test]
fn every_slow_fixture_is_answered_by_the_decoder_without_a_panic() -> R {
    for file in files("jpeg-slow-")? {
        let bytes = std::fs::read(dir().join(&file))?;
        let answered = std::panic::catch_unwind(|| decode(Format::Jpeg, &bytes));
        assert!(answered.is_ok(), "{file}: the decoder panicked");
    }
    Ok(())
}
