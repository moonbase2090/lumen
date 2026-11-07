//! A WebP whose prefix-code lengths oversubscribe the code, far enough that
//! `image-webp` 0.2.4 would sum the canonical codes past 16 bits (`huffman.rs`,
//! `build_implicit`). With overflow checks the library panics there; in a
//! release build the sum wraps and the completeness test after the loop rejects
//! the code. The decoder checks every prefix code before the library sees the
//! stream, so both builds answer `Corrupt` (status 4) and neither panics.
//! These tests run in debug builds (overflow checks on, as in the fuzz build)
//! and with `cargo test --release`.
//!
//! The streams are synthesized: a one-pixel VP8L image whose green prefix code
//! has a chosen code-length histogram. Inputs found by fuzzing stay in private
//! storage; point `LUMEN_PRIVATE_WEBP_FIXTURE` at one file, or
//! `LUMEN_PRIVATE_WEBP_FIXTURE_DIR` at a directory whose `crash-*` files are
//! such inputs, to check them here too.

#![allow(missing_docs)]

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::Format;
use lumen_decoder::protocol::Status;

/// The green alphabet (256 literals + 24 lengths) plus an 11-bit colour cache.
const GREEN_SYMBOLS: usize = 280 + (1 << 11);

/// `(code length, symbol count)`, shortest first. Three codes of length 1 leave
/// the code oversubscribed; 2000 codes of length 15 then carry the sum past
/// 65535, and the wrapped value is not the one a complete code ends on.
const OVERSUBSCRIBED: [(u16, usize); 6] = [(1, 3), (2, 1), (3, 1), (4, 1), (5, 1), (15, 2000)];

/// The environment variable that names a private fixture.
const PRIVATE_FIXTURE: &str = "LUMEN_PRIVATE_WEBP_FIXTURE";
/// The environment variable that names a directory of private `crash-*` fixtures.
const PRIVATE_FIXTURE_DIR: &str = "LUMEN_PRIVATE_WEBP_FIXTURE_DIR";

#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    /// The low `n` bits of `value`, least significant first.
    fn put(&mut self, value: u32, n: usize) {
        for i in 0..n {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1
                && let Some(last) = self.bytes.last_mut()
            {
                *last |= 1 << (self.used % 8);
            }
            self.used += 1;
        }
    }

    /// A prefix code of `len` bits, most significant bit first.
    fn put_code(&mut self, code: u32, len: usize) {
        for i in (0..len).rev() {
            self.put((code >> i) & 1, 1);
        }
    }
}

/// Canonical codes `(code, length)` of a complete code over `k` symbols with
/// lengths `1, 2, .., k - 1, k - 1`: the code-length alphabet of the stream.
fn chain_codes(k: usize) -> Vec<(u32, usize)> {
    let mut codes = Vec::new();
    let mut code = 0u32;
    let mut prev = 1usize;
    for i in 0..k {
        let len = (i + 1).min(k - 1).max(1);
        code <<= len - prev;
        codes.push((code, len));
        code += 1;
        prev = len;
    }
    codes
}

/// A one-pixel VP8L image whose green prefix code has `histogram`'s lengths.
fn webp_with_green_lengths(histogram: &[(u16, usize)]) -> Vec<u8> {
    let mut lengths: Vec<u16> = Vec::new();
    for &(len, count) in histogram {
        lengths.extend(std::iter::repeat_n(len, count));
    }
    lengths.resize(GREEN_SYMBOLS, 0);

    let mut values = lengths.clone();
    values.sort_unstable();
    values.dedup();
    let codes = chain_codes(values.len());
    let code_of = |v: u16| values.iter().position(|&x| x == v).map(|i| codes[i]);

    let mut b = Bits::default();
    b.put(0x2f, 8); // signature
    b.put(0, 14); // width - 1
    b.put(0, 14); // height - 1
    b.put(0, 1); // alpha hint
    b.put(0, 3); // version
    b.put(0, 1); // no transform
    b.put(1, 1); // colour cache
    b.put(11, 4); // of 11 bits
    b.put(0, 1); // no meta prefix codes

    // Green: a normal prefix code, with all 19 code-length code lengths.
    b.put(0, 1);
    b.put(15, 4);
    for sym in [
        17u16, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    ] {
        let len = code_of(sym).map_or(0, |(_, n)| n);
        b.put(u32::try_from(len).unwrap_or(0), 3);
    }
    b.put(0, 1); // every symbol has a length
    for &len in &lengths {
        if let Some((code, n)) = code_of(len) {
            b.put_code(code, n);
        }
    }

    // Red, blue, alpha and distance: one symbol each.
    for _ in 0..4 {
        b.put(0b1, 1); // simple
        b.put(0, 1); // one symbol
        b.put(0, 1); // a 1-bit symbol
        b.put(0, 1); // which is 0
    }
    b.put(0, 32); // pixel data

    let payload_len = u32::try_from(b.bytes.len()).unwrap_or(0);
    let mut payload = b.bytes;
    if payload.len() % 2 == 1 {
        payload.push(0);
    }
    let mut out = b"RIFF".to_vec();
    out.extend((12 + u32::try_from(payload.len()).unwrap_or(0)).to_le_bytes());
    out.extend(b"WEBPVP8L");
    out.extend(payload_len.to_le_bytes());
    out.extend(payload);
    out
}

/// The size a lossless stream declares, which the host passes as its hint.
#[cfg(all(unix, not(debug_assertions)))]
fn vp8l_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let h = bytes.get(21..25)?;
    let (b0, b1, b2, b3) = (
        u32::from(h[0]),
        u32::from(h[1]),
        u32::from(h[2]),
        u32::from(h[3]),
    );
    Some((
        ((b1 & 0x3f) << 8 | b0) + 1,
        ((b3 & 0xf) << 10 | b2 << 2 | b1 >> 6) + 1,
    ))
}

fn private_fixture() -> Option<Vec<u8>> {
    std::fs::read(std::env::var_os(PRIVATE_FIXTURE)?).ok()
}

/// Every `crash-*` file in the private fixture directory.
fn private_crash_fixtures() -> Vec<(String, Vec<u8>)> {
    let Some(dir) = std::env::var_os(PRIVATE_FIXTURE_DIR) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("crash-")
            && let Ok(bytes) = std::fs::read(entry.path())
        {
            found.push((name, bytes));
        }
    }
    found.sort();
    found
}

#[test]
fn an_oversubscribed_code_is_rejected_as_corrupt() {
    let bytes = webp_with_green_lengths(&OVERSUBSCRIBED);
    assert_eq!(
        decode(Format::Webp, &bytes).map(|i| i.width),
        Err(Status::Corrupt)
    );
}

/// Other oversubscribed histograms, each a different way to push the sum past
/// 16 bits or to leave the code over-full without doing so.
#[test]
fn every_oversubscribed_histogram_is_rejected_as_corrupt() {
    let histograms: [&[(u16, usize)]; 5] = [
        &[(1, 3), (2, 1), (3, 1), (4, 1), (5, 1), (15, 2000)],
        &[(1, 2), (15, 2000)],
        &[(2, 5)],
        &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (15, 2200)],
        &[(15, 2300)],
    ];
    for histogram in histograms {
        let bytes = webp_with_green_lengths(histogram);
        assert_eq!(
            decode(Format::Webp, &bytes).map(|i| i.width),
            Err(Status::Corrupt),
            "{histogram:?}"
        );
    }
}

/// A RIFF file around `chunks`, with the size field filled in.
fn riff(chunks: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend((4 + u32::try_from(chunks.len()).unwrap_or(0)).to_le_bytes());
    out.extend(b"WEBP");
    out.extend(chunks);
    out
}

/// A chunk: its tag, its size, the body, and a pad byte for an odd size.
fn chunk(tag: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend(u32::try_from(body.len()).unwrap_or(0).to_le_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// The same stream behind an extended header and as an animation frame.
#[test]
fn an_oversubscribed_code_is_rejected_in_every_container() {
    let simple = webp_with_green_lengths(&OVERSUBSCRIBED);
    let vp8l = simple[12..].to_vec();
    let payload = vp8l[8..].to_vec();

    let canvas = |flags: u8| chunk(*b"VP8X", &[flags, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let mut extended = canvas(0x10);
    extended.extend(&vp8l);

    let mut frame = vec![0u8; 12];
    frame.extend([100, 0, 0, 0]);
    frame.extend(chunk(*b"VP8L", &payload));
    let mut animated = canvas(0x12);
    animated.extend(chunk(*b"ANIM", &[0, 0, 0, 0, 0, 0]));
    animated.extend(chunk(*b"ANMF", &frame));

    for (name, bytes) in [("extended", extended), ("animated", animated)] {
        assert_eq!(
            decode(Format::Webp, &riff(&bytes)).map(|i| i.width),
            Err(Status::Corrupt),
            "{name}"
        );
    }
}

/// A well-formed code of the same shape still decodes: the check rejects only
/// what the library rejects.
#[test]
fn a_complete_code_still_decodes() {
    // Two 1-bit codes fill the code exactly.
    let bytes = webp_with_green_lengths(&[(1, 2)]);
    let outcome = decode(Format::Webp, &bytes);
    assert!(outcome.is_ok(), "{:?}", outcome.map(|i| i.width));
}

#[test]
fn a_private_fixture_is_rejected_as_corrupt() {
    let Some(bytes) = private_fixture() else {
        eprintln!("{PRIVATE_FIXTURE} is not set: no private fixture checked");
        return;
    };
    assert_eq!(
        decode(Format::Webp, &bytes).map(|i| i.width),
        Err(Status::Corrupt)
    );
}

#[test]
fn private_crash_fixtures_are_rejected_without_a_panic() {
    let fixtures = private_crash_fixtures();
    if fixtures.is_empty() {
        eprintln!("{PRIVATE_FIXTURE_DIR} is not set: no private fixtures checked");
    }
    for (name, bytes) in fixtures {
        let outcome = std::panic::catch_unwind(|| decode(Format::Webp, &bytes).map(|i| i.width));
        assert_eq!(outcome.ok(), Some(Err(Status::Corrupt)), "{name}");
    }
}

/// The real worker, as the host runs it: the answer is status 4, not a crash.
#[cfg(all(unix, not(debug_assertions)))]
mod worker {
    use std::time::Duration;

    use lumen_decoder::runner::{Outcome, Runner};

    use super::*;

    const WORKER: &str = env!("CARGO_BIN_EXE_lumen-decoder-worker");

    fn rejected(bytes: &[u8]) -> Outcome {
        Runner::new(WORKER)
            .with_wall(Duration::from_secs(60))
            .decode(Format::Webp, (1, 1), bytes)
    }

    #[test]
    fn the_release_worker_answers_status_four_for_an_oversubscribed_code() {
        let bytes = webp_with_green_lengths(&OVERSUBSCRIBED);
        assert_eq!(rejected(&bytes), Outcome::Rejected(Status::Corrupt));
    }

    #[test]
    fn the_release_worker_answers_status_four_for_a_private_fixture() {
        let Some(bytes) = private_fixture() else {
            eprintln!("{PRIVATE_FIXTURE} is not set: no private fixture checked");
            return;
        };
        let outcome = Runner::new(WORKER)
            .with_wall(Duration::from_secs(60))
            .decode(Format::Webp, vp8l_size(&bytes).unwrap_or((1, 1)), &bytes);
        assert_eq!(outcome, Outcome::Rejected(Status::Corrupt));
    }
}
