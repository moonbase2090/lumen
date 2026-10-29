//! JPEG decoding of the small regression seeds, so Miri can run the shipped
//! `zune-jpeg` code (its `unsafe` SIMD and bit reader) on real inputs:
//!
//! ```sh
//! MIRIFLAGS=-Zmiri-disable-isolation cargo +nightly miri test -p lumen-decoder-tools --test miri_jpeg
//! ```
//!
//! Under plain `cargo test` it is an ordinary test. Only files up to 2 KiB are
//! used, because Miri is slow.

#![allow(missing_docs)]

use std::error::Error;
use std::path::PathBuf;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::Format;

#[test]
fn small_regression_jpegs_decode_or_are_rejected() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../lumen-decoder/fuzz/seeds-regression/jpeg");
    let mut seen = 0;
    let mut decoded = 0;
    for entry in std::fs::read_dir(dir)? {
        let bytes = std::fs::read(entry?.path())?;
        if bytes.len() > 2048 {
            continue;
        }
        seen += 1;
        if decode(Format::Jpeg, &bytes).is_ok() {
            decoded += 1;
        }
    }
    assert!(seen >= 5, "{seen}");
    assert!(decoded >= 4, "{decoded} of {seen}");
    Ok(())
}
