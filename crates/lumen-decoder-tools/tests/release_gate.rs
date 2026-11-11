//! The release's gate evidence, committed in `release/`, is for this tree: the
//! build identities in the ledger and in the gate records are the identities of
//! the sources, the ledger passes the judge, and every platform has a record
//! with both gates passed. A change to a decoder or image-path source or to the
//! lockfile changes an identity and fails this test until the records are
//! redone, which is the re-fuzz rule (release criteria, rule 5) applied.

#![allow(missing_docs)]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use lumen_decoder_tools::identity::{self, Kind};
use lumen_evidence::decoder::{Report, judge};
use serde_json::Value;

type R = Result<(), Box<dyn Error>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source_identity(kind: Kind) -> Result<String, Box<dyn Error>> {
    let root = root();
    let files = identity::files(&root, kind)?;
    Ok(identity::hash(&root, &files, "", identity::SOURCE_ONLY)?)
}

/// platform, decoder identity, image-path identity, decoder gate, memory gate.
type Record = Vec<String>;

fn records() -> Result<Vec<Record>, Box<dyn Error>> {
    let text = fs::read_to_string(root().join("release/gate-records.txt"))?;
    Ok(text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(str::to_owned).collect())
        .collect())
}

fn ledger_text() -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        root().join("release/decoder-ledger.json"),
    )?)
}

#[test]
fn the_ledger_is_for_the_identities_of_this_tree() -> R {
    let ledger: Value = serde_json::from_str(&ledger_text()?)?;
    assert_eq!(
        ledger["decoder_build"],
        source_identity(Kind::Decoder)?,
        "a decoder source or the lockfile changed: the decoder gate has to be redone for the new identity"
    );
    assert_eq!(
        ledger["image_path_build"],
        source_identity(Kind::ImagePath)?,
        "an image-path source or the lockfile changed: the gate has to be redone for the new identity"
    );
    let flags = fs::read_to_string(root().join("release/decoder-fuzz-flags.txt"))?;
    assert_eq!(flags.trim(), identity::SOURCE_ONLY);
    Ok(())
}

#[test]
fn every_platform_has_a_record_for_this_tree_with_both_gates_passed() -> R {
    let decoder = source_identity(Kind::Decoder)?;
    let path = source_identity(Kind::ImagePath)?;
    let records = records()?;
    assert_eq!(records.len(), 3, "one record for each platform, no other");
    for platform in ["linux", "macos", "windows"] {
        let mine: Vec<&Record> = records.iter().filter(|r| r[0] == platform).collect();
        assert_eq!(mine.len(), 1, "one record for {platform}");
        let r = mine[0];
        assert_eq!(r.len(), 5, "{platform} has five fields");
        assert_eq!((&r[1], &r[2]), (&decoder, &path), "{platform} identities");
        assert_eq!(
            (r[3].as_str(), r[4].as_str()),
            ("pass", "pass"),
            "{platform}"
        );
    }
    Ok(())
}

#[test]
fn the_ledger_passes_the_judge_for_all_three_formats() -> R {
    let report = judge(&ledger_text()?)?;
    assert!(
        report.passes(),
        "the judge did not pass the committed ledger"
    );
    let Report::Judged { formats, .. } = report else {
        return Err("the ledger was not judged".into());
    };
    let names: Vec<&str> = formats.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["png", "jpeg", "webp"]);
    assert!(formats.iter().all(|f| f.pass && f.open == 0));
    Ok(())
}

#[test]
fn the_command_line_gives_the_same_source_identity_as_the_library() -> R {
    let run = |kind: &str| -> Result<String, Box<dyn Error>> {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-decoder-tools"))
            .arg("identity")
            .arg(root())
            .arg(identity::SOURCE_ONLY)
            .arg(kind)
            .output()?;
        assert!(out.status.success());
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    assert_eq!(run("decoder")?, source_identity(Kind::Decoder)?);
    assert_eq!(run("image-path")?, source_identity(Kind::ImagePath)?);
    Ok(())
}

#[test]
fn a_changed_source_does_not_match_the_committed_identity() -> R {
    // The committed identity is for these exact bytes: hashing the same files
    // with the compiler left in, or with other flags, gives another identity.
    let root = root();
    let files = identity::files(&root, Kind::Decoder)?;
    let source = identity::hash(&root, &files, "", identity::SOURCE_ONLY)?;
    assert_ne!(
        source,
        identity::hash(&root, &files, "rustc 1", identity::SOURCE_ONLY)?
    );
    assert_ne!(source, identity::hash(&root, &files, "", "-Cdebuginfo=2")?);
    Ok(())
}
