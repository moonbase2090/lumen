//! `zune-jpeg` 0.5.15 panics on malformed JPEGs (slice indexing in its scalar
//! inverse DCT, `idct/scalar.rs`): the fuzzer found 16 distinct inputs within two
//! CPU-hours, and every one of them made the production worker exit with status
//! 101 and no reply. 0.5.16-rc3 does not panic on any of them. The fixtures are
//! those inputs, with the JPEG signature in front where the fuzz target had added
//! it. Nothing in the decoder catches a panic: this is the library's behaviour.

#![allow(missing_docs)]

use std::error::Error;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::{Format, Status};

/// A fixture's file name and its bytes.
type Fixture = (String, Vec<u8>);

fn fixtures() -> Result<Vec<Fixture>, Box<dyn Error>> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if name.starts_with("jpeg-panic-") {
            out.push((name, std::fs::read(&path)?));
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn there_are_sixteen_reproducers_and_each_is_a_jpeg() -> Result<(), Box<dyn Error>> {
    let all = fixtures()?;
    assert_eq!(all.len(), 16);
    for (name, bytes) in &all {
        assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "{name}");
        assert_eq!(Format::sniff(bytes), Some(Format::Jpeg), "{name}");
    }
    Ok(())
}

#[test]
fn a_malformed_jpeg_that_panicked_zune_jpeg_0_5_15_is_answered_not_a_panic()
-> Result<(), Box<dyn Error>> {
    let mut answered = std::collections::BTreeMap::new();
    for (name, bytes) in fixtures()? {
        // A panic here is the failure this test exists for.
        let result = std::panic::catch_unwind(|| decode(Format::Jpeg, &bytes));
        let result = result.map_err(|_| format!("{name}: the decoder panicked"))?;
        let label = match result {
            Ok(img) => {
                assert_eq!(
                    img.rgba.len(),
                    img.width as usize * img.height as usize * 4,
                    "{name}"
                );
                "decoded"
            }
            Err(Status::Corrupt) => "rejected: corrupt",
            Err(Status::TooLarge) => "rejected: too large",
            Err(other) => return Err(format!("{name}: unexpected status {other:?}").into()),
        };
        *answered.entry(label).or_insert(0u32) += 1;
    }
    eprintln!("jpeg reproducers: {answered:?}");
    assert_eq!(answered.values().sum::<u32>(), 16);
    Ok(())
}
