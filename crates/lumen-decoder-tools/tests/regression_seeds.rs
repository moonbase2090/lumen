//! The regression seeds are the upstream projects' own inputs for bugs they
//! fixed, pinned by commit and hash. Each must be the file the manifest says,
//! and the real decoder must meet each with a decode or a rejection, never a
//! panic. (They are not CVE reproducers: the chosen libraries have no
//! registered advisories.)

#![allow(missing_docs)]

use std::error::Error;
use std::path::PathBuf;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::{Format, Status};
use sha2::{Digest, Sha256};

type R = Result<(), Box<dyn Error>>;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lumen-decoder/fuzz/seeds-regression")
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

struct Row {
    file: String,
    bytes: usize,
    sha256: String,
    commit: String,
}

fn manifest() -> Result<Vec<Row>, Box<dyn Error>> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.tsv"))?;
    text.lines()
        .skip(1)
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() != 7 {
                return Err(format!("bad manifest row: {l}").into());
            }
            Ok(Row {
                file: c[0].into(),
                commit: c[2].into(),
                bytes: c[4].parse()?,
                sha256: c[5].into(),
            })
        })
        .collect()
}

#[test]
fn every_file_is_the_one_the_manifest_names() -> R {
    let rows = manifest()?;
    assert!(rows.len() >= 30);
    for r in &rows {
        let bytes = std::fs::read(dir().join(&r.file))?;
        assert_eq!(bytes.len(), r.bytes, "{}", r.file);
        assert_eq!(hex(&bytes), r.sha256, "{}", r.file);
        assert_eq!(r.commit.len(), 40, "{} is pinned to a full commit", r.file);
    }
    // No file is present that the manifest does not list.
    let mut on_disk = 0;
    for f in ["png", "jpeg", "webp"] {
        on_disk += std::fs::read_dir(dir().join(f))?.count();
    }
    assert_eq!(on_disk, rows.len());
    Ok(())
}

#[test]
fn the_real_decoder_decodes_or_rejects_each_and_never_panics() -> R {
    let mut tally = std::collections::BTreeMap::new();
    for r in manifest()? {
        let format = match r.file.split('/').next() {
            Some("png") => Format::Png,
            Some("jpeg") => Format::Jpeg,
            _ => Format::Webp,
        };
        let bytes = std::fs::read(dir().join(&r.file))?;
        // A panic fails the test; every other outcome is a clean answer.
        let outcome = match decode(format, &bytes) {
            Ok(img) => {
                assert_eq!(
                    img.rgba.len(),
                    img.width as usize * img.height as usize * 4,
                    "{}",
                    r.file
                );
                "decoded"
            }
            Err(Status::Corrupt) => "rejected: corrupt",
            Err(Status::TooLarge) => "rejected: too large",
            Err(Status::Unsupported) => "rejected: unsupported",
            Err(other) => return Err(format!("{}: unexpected status {other:?}", r.file).into()),
        };
        *tally.entry(outcome).or_insert(0u32) += 1;
    }
    eprintln!("regression seeds: {tally:?}");
    assert!(
        tally.get("decoded").copied().unwrap_or(0) >= 10,
        "{tally:?}"
    );
    Ok(())
}
