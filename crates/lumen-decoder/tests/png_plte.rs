//! `png` 0.18.1 panics on a `PLTE` chunk whose length is not a multiple of 3
//! (fixed upstream, not released). The decoder rejects such files first. The
//! fixtures are inputs the fuzzer found, with their checksums repaired so a
//! normal build reaches the panic; each has such a chunk.

#![allow(missing_docs)]

use std::error::Error;
use std::io::Cursor;

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::{Format, Status};

const FIXTURES: [&str; 4] = [
    "png-plte-length-2b2d5c66.png",
    "png-plte-length-8ace8031.png",
    "png-plte-length-d007b71b.png",
    "png-plte-length-f3e24d20.png",
];

fn fixture(name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))?)
}

#[test]
fn a_plte_chunk_that_is_not_a_multiple_of_three_is_rejected_not_a_panic()
-> Result<(), Box<dyn Error>> {
    for name in FIXTURES {
        let bytes = fixture(name)?;
        assert_eq!(
            decode(Format::Png, &bytes).map(|i| i.width),
            Err(Status::Corrupt),
            "{name}"
        );
    }
    Ok(())
}

/// Drives `png` directly, with the same transformations the decoder uses.
fn png_alone(bytes: &[u8]) {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    if let Ok(mut reader) = dec.read_info()
        && let Some(size) = reader.output_buffer_size()
    {
        let mut buf = vec![0u8; size];
        let _ = reader.next_frame(&mut buf);
    }
}

#[test]
fn png_still_panics_on_the_fixtures_without_the_check() {
    // When this fails, the `png` in use has the upstream fix: delete
    // `plte_lengths_ok` in src/decode.rs and this file.
    let panicking = FIXTURES
        .iter()
        .filter(|name| {
            let bytes = fixture(name).unwrap_or_default();
            std::panic::catch_unwind(|| png_alone(&bytes)).is_err()
        })
        .count();
    assert_eq!(
        panicking,
        FIXTURES.len(),
        "png no longer panics on {} of the fixtures",
        FIXTURES.len() - panicking
    );
}

#[test]
fn a_valid_palette_png_still_decodes() -> Result<(), Box<dyn Error>> {
    // A 2x1 indexed image with a 2-entry palette (6 bytes, a multiple of 3).
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 2, 1);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(vec![255, 0, 0, 0, 255, 0]);
        let mut w = enc.write_header()?;
        w.write_image_data(&[0, 1])?;
    }
    let img = decode(Format::Png, &out).map_err(|s| format!("{s:?}"))?;
    assert_eq!(&img.rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
    Ok(())
}
