//! Seeded mutations of real images through the real decoder.
//!
//! The fuzz campaign runs for hours on a VM and its findings are checked in as
//! regression inputs. This is the part that runs on every change: each
//! regression seed is damaged in many seeded ways (flipped bytes, truncation,
//! removed and repeated ranges, and the declared size rewritten to extremes), the
//! checksums of a PNG are repaired so the damage reaches the decoder instead of
//! the checksum, and the decoder must answer each with a decode or a refusal,
//! never a panic. A decode must obey what the rest of the terminal relies on: the
//! size inside the limits, exactly four bytes a pixel, and premultiplied colour.

#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation)]

use std::path::PathBuf;

use lumen_decoder::decode::decode;
use lumen_decoder::limits::{MAX_PIXELS, MAX_SIDE};
use lumen_decoder::protocol::Format;

const ROUNDS: usize = 150;

/// Mutations per seed; `LUMEN_SWEEP_ROUNDS` raises it for a longer local run.
fn rounds() -> usize {
    std::env::var("LUMEN_SWEEP_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(ROUNDS)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Recomputes the checksum of every PNG chunk whose length fits the file.
fn repair_png(bytes: &mut [u8]) {
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let len =
            u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let Some(end) = (at + 8).checked_add(len).filter(|e| e + 4 <= bytes.len()) else {
            return;
        };
        let sum = crc32(&bytes[at + 4..end]);
        bytes[end..end + 4].copy_from_slice(&sum.to_be_bytes());
        at = end + 4;
    }
}

fn extreme(rng: &mut Rng) -> u32 {
    [
        0,
        1,
        2,
        255,
        256,
        4096,
        65_535,
        65_536,
        0x7FFF_FFFF,
        u32::MAX,
    ][rng.below(10)]
}

fn mutate(format: Format, seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = seed.to_vec();
    if format == Format::Png && b.len() > 20 && rng.below(4) == 0 {
        // Resize one chunk's data, keeping the chunks after it in place, as the
        // fuzzer's finds did (a palette whose length is not a multiple of 3).
        let mut starts = Vec::new();
        let mut at = 8;
        while at + 12 <= b.len() {
            let len = u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize;
            if at + 12 + len > b.len() {
                break;
            }
            starts.push((at, len));
            at += 12 + len;
        }
        if starts.is_empty() {
            return b;
        }
        let (at, len) = starts[rng.below(starts.len())];
        let new_len = rng.below(40);
        let mut data = b[at + 8..at + 8 + len].to_vec();
        data.resize(new_len, 0);
        let mut chunk = (new_len as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(&b[at + 4..at + 8]);
        chunk.extend_from_slice(&data);
        chunk.extend_from_slice(&[0; 4]);
        b.splice(at..at + 12 + len, chunk);
        repair_png(&mut b);
        return b;
    }
    match rng.below(6) {
        0 | 1 => {
            for _ in 0..=rng.below(4) {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
        }
        2 => b.truncate(rng.below(b.len())),
        3 => {
            let from = rng.below(b.len());
            let to = (from + 1 + rng.below(64)).min(b.len());
            b.drain(from..to);
        }
        4 => {
            let from = rng.below(b.len());
            let to = (from + 1 + rng.below(64)).min(b.len());
            let part = b[from..to].to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, part);
        }
        _ => match format {
            // The declared size of a PNG is in the header chunk.
            Format::Png if b.len() >= 24 => {
                b[16..20].copy_from_slice(&extreme(rng).to_be_bytes());
                b[20..24].copy_from_slice(&extreme(rng).to_be_bytes());
            }
            // The declared size of a JPEG is in its start-of-frame marker.
            Format::Jpeg => {
                if let Some(i) = b
                    .windows(2)
                    .position(|w| w[0] == 0xFF && matches!(w[1], 0xC0..=0xC2))
                    && i + 9 <= b.len()
                {
                    b[i + 5..i + 7].copy_from_slice(&(extreme(rng) as u16).to_be_bytes());
                    b[i + 7..i + 9].copy_from_slice(&(extreme(rng) as u16).to_be_bytes());
                }
            }
            _ => {
                let i = rng.below(b.len());
                b[i] = rng.next() as u8;
            }
        },
    }
    if format == Format::Png && b.len() > 8 {
        repair_png(&mut b);
    }
    b
}

fn seeds() -> Vec<(Format, String, Vec<u8>)> {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lumen-decoder/fuzz/seeds-regression");
    let mut out = Vec::new();
    for (dir, format) in [
        ("png", Format::Png),
        ("jpeg", Format::Jpeg),
        ("webp", Format::Webp),
    ] {
        let mut names: Vec<_> = std::fs::read_dir(root.join(dir))
            .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        names.sort();
        for path in names {
            if let Ok(bytes) = std::fs::read(&path) {
                out.push((
                    format,
                    format!(
                        "{dir}/{}",
                        path.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    ),
                    bytes,
                ));
            }
        }
    }
    out
}

#[test]
fn damaged_images_are_answered_never_a_panic_and_a_decode_obeys_its_contract() {
    let seeds = seeds();
    assert!(seeds.len() >= 30, "the regression seeds were not found");
    let mut decoded = 0usize;
    let mut refused = 0usize;
    let mut failures = Vec::new();
    for (n, (format, name, seed)) in seeds.iter().enumerate() {
        if seed.is_empty() {
            continue;
        }
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((n as u64 + 1) << 20));
        for round in 0..rounds() {
            let input = mutate(*format, seed, &mut rng);
            let format = *format;
            let result = std::panic::catch_unwind(|| decode(format, &input));
            match result {
                Err(_) => failures.push(format!("{name} round {round}: the decoder panicked")),
                Ok(Err(_)) => refused += 1,
                Ok(Ok(d)) => {
                    decoded += 1;
                    let (w, h) = (u64::from(d.width), u64::from(d.height));
                    let ok = w >= 1
                        && h >= 1
                        && w <= MAX_SIDE
                        && h <= MAX_SIDE
                        && w * h <= MAX_PIXELS
                        && d.rgba.len() as u64 == w * h * 4
                        && d.rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
                    if !ok {
                        failures.push(format!(
                            "{name} round {round}: a decode broke its contract ({}x{}, {} bytes)",
                            d.width,
                            d.height,
                            d.rgba.len()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // The sweep must reach both outcomes, or it tests nothing.
    assert!(decoded > 500, "only {decoded} damaged inputs still decoded");
    assert!(refused > 500, "only {refused} damaged inputs were refused");
}
