//! A WebP whose prefix-code lengths oversubscribe the code, far enough that
//! `image-webp` 0.2.4 sums the canonical codes past 16 bits (`huffman.rs`,
//! `build_implicit`). With overflow checks the library panics there; in a
//! release build the sum wraps and the completeness test after the loop rejects
//! the code, so the decoder answers `Corrupt` (status 4). The release test is
//! the proof the production worker takes that path; CI runs it with
//! `cargo test --release`.
//!
//! The stream is synthesized: a one-pixel VP8L image whose green prefix code has
//! a chosen code-length histogram. Inputs found by fuzzing stay in private
//! storage; point `LUMEN_PRIVATE_WEBP_FIXTURE` at one to check it here too.

#![allow(missing_docs)]

use lumen_decoder::decode::decode;
use lumen_decoder::protocol::Format;
#[cfg(not(debug_assertions))]
use lumen_decoder::protocol::Status;

/// The green alphabet (256 literals + 24 lengths) plus an 11-bit colour cache.
const GREEN_SYMBOLS: usize = 280 + (1 << 11);

/// `(code length, symbol count)`, shortest first. Three codes of length 1 leave
/// the code oversubscribed; 2000 codes of length 15 then carry the sum past
/// 65535, and the wrapped value is not the one a complete code ends on.
const OVERSUBSCRIBED: [(u16, usize); 6] = [(1, 3), (2, 1), (3, 1), (4, 1), (5, 1), (15, 2000)];

/// The environment variable that names a private fixture.
#[cfg(not(debug_assertions))]
const PRIVATE_FIXTURE: &str = "LUMEN_PRIVATE_WEBP_FIXTURE";

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

#[cfg(not(debug_assertions))]
fn private_fixture() -> Option<Vec<u8>> {
    std::fs::read(std::env::var_os(PRIVATE_FIXTURE)?).ok()
}

#[cfg(not(debug_assertions))]
#[test]
fn an_oversubscribed_code_is_rejected_as_corrupt_in_release() {
    let bytes = webp_with_green_lengths(&OVERSUBSCRIBED);
    assert_eq!(
        decode(Format::Webp, &bytes).map(|i| i.width),
        Err(Status::Corrupt)
    );
}

#[cfg(not(debug_assertions))]
#[test]
fn a_private_fixture_is_rejected_as_corrupt_in_release() {
    let Some(bytes) = private_fixture() else {
        eprintln!("{PRIVATE_FIXTURE} is not set: no private fixture checked");
        return;
    };
    assert_eq!(
        decode(Format::Webp, &bytes).map(|i| i.width),
        Err(Status::Corrupt)
    );
}

#[cfg(debug_assertions)]
#[test]
fn an_oversubscribed_code_panics_in_the_library_with_overflow_checks() {
    // When this fails, the `image-webp` in use no longer overflows here.
    let bytes = webp_with_green_lengths(&OVERSUBSCRIBED);
    let outcome = std::panic::catch_unwind(|| decode(Format::Webp, &bytes).map(|i| i.width));
    assert!(outcome.is_err());
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
