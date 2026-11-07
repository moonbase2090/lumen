//! A pre-decode check for lossless WebP prefix codes.
//!
//! `image-webp` 0.2.4 sums the canonical codes of a prefix code in 16 bits
//! (`huffman.rs`, `build_implicit`). A code whose lengths oversubscribe the
//! code can carry that sum past 65535: with overflow checks (debug builds and
//! the fuzz build) the library panics, and in a release build the sum wraps and
//! a later completeness test rejects the code. The upstream fix is not in a
//! release yet.
//!
//! This module walks every lossless stream in the file the way the library
//! does, reads each prefix code's lengths, and returns [`Status::Corrupt`] for a
//! code the library would reject, before the library sees the stream. It never
//! rejects a file for any other reason: when the stream does not parse
//! further (a truncated stream, an unknown chunk, a bad transform) it stops
//! and lets the library give its own answer.
//!
//! It is on for every decode and has no switch.
//!
//! TEMPORARY. When `image-webp` releases the fix: bump the pin in
//! `Cargo.toml`, delete this module and its call in `decode.rs`, and keep the
//! regression tests in `tests/webp_guard.rs`.

use crate::protocol::Status;

/// The code-length alphabet: 16 literal lengths and three repeat codes.
const CODE_LENGTH_CODES: usize = 19;
const CODE_LENGTH_CODE_ORDER: [usize; CODE_LENGTH_CODES] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
/// Green (256 literals and 24 length prefixes), red, blue, alpha, distance.
const ALPHABET_SIZE: [usize; 5] = [256 + 24, 256, 256, 256, 40];
const MAX_CODE_LENGTH: usize = 15;
/// Sub-images (transform data, palette, entropy image) are at most this many
/// pixels in any file the decoder accepts. A larger one is left to the library.
const MAX_SUB_IMAGE_PIXELS: usize = 1 << 22;

/// Offsets `(dx, dy)` of the 120 short distance codes (the format's table).
#[rustfmt::skip]
const DISTANCE_MAP: [(i8, i8); 120] = [
    (0, 1),  (1, 0),  (1, 1),  (-1, 1), (0, 2),  (2, 0),  (1, 2),  (-1, 2),
    (2, 1),  (-2, 1), (2, 2),  (-2, 2), (0, 3),  (3, 0),  (1, 3),  (-1, 3),
    (3, 1),  (-3, 1), (2, 3),  (-2, 3), (3, 2),  (-3, 2), (0, 4),  (4, 0),
    (1, 4),  (-1, 4), (4, 1),  (-4, 1), (3, 3),  (-3, 3), (2, 4),  (-2, 4),
    (4, 2),  (-4, 2), (0, 5),  (3, 4),  (-3, 4), (4, 3),  (-4, 3), (5, 0),
    (1, 5),  (-1, 5), (5, 1),  (-5, 1), (2, 5),  (-2, 5), (5, 2),  (-5, 2),
    (4, 4),  (-4, 4), (3, 5),  (-3, 5), (5, 3),  (-5, 3), (0, 6),  (6, 0),
    (1, 6),  (-1, 6), (6, 1),  (-6, 1), (2, 6),  (-2, 6), (6, 2),  (-6, 2),
    (4, 5),  (-4, 5), (5, 4),  (-5, 4), (3, 6),  (-3, 6), (6, 3),  (-6, 3),
    (0, 7),  (7, 0),  (1, 7),  (-1, 7), (5, 5),  (-5, 5), (7, 1),  (-7, 1),
    (4, 6),  (-4, 6), (6, 4),  (-6, 4), (2, 7),  (-2, 7), (7, 2),  (-7, 2),
    (3, 7),  (-3, 7), (7, 3),  (-7, 3), (5, 6),  (-5, 6), (6, 5),  (-6, 5),
    (8, 0),  (4, 7),  (-4, 7), (7, 4),  (-7, 4), (8, 1),  (8, 2),  (6, 6),
    (-6, 6), (8, 3),  (5, 7),  (-5, 7), (7, 5),  (-7, 5), (8, 4),  (6, 7),
    (-6, 7), (7, 6),  (-7, 6), (8, 5),  (7, 7),  (-7, 7), (8, 6),  (8, 7)
];

/// Why the walk stopped.
#[derive(Debug, PartialEq, Eq)]
enum Stop {
    /// A prefix code the library rejects (and, with overflow checks, panics on).
    BadCode,
    /// Anything else: the library gives its own answer.
    Unparsed,
}

type Walk<T> = Result<T, Stop>;

/// Checks every lossless stream in a WebP file. `Err(Status::Corrupt)` means a
/// prefix code is invalid; the file must not reach the library.
pub(crate) fn precheck(bytes: &[u8]) -> Result<(), Status> {
    match check_file(bytes) {
        Err(Stop::BadCode) => Err(Status::Corrupt),
        Ok(()) | Err(Stop::Unparsed) => Ok(()),
    }
}

fn check_file(bytes: &[u8]) -> Walk<()> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(Stop::Unparsed);
    }
    check_chunks(bytes.get(12..).ok_or(Stop::Unparsed)?, false)
}

/// Walks a run of chunks the way the library does: it uses the first lossless,
/// alpha and lossy chunk, and for an animation only the first frame, whose data
/// is itself a run of chunks (the library looks at its first two). Streams the
/// library never decodes are not checked, so a bad later frame does not stop a
/// file whose first frame decodes.
fn check_chunks(mut rest: &[u8], in_frame: bool) -> Walk<()> {
    // Alpha data takes its size from the lossy frame beside it.
    let mut alpha: Option<&[u8]> = None;
    let mut frame_size: Option<(usize, usize)> = None;
    let (mut seen_lossless, mut seen_frame, mut seen_alpha, mut seen_lossy) =
        (false, false, false, false);
    let mut chunks = 0;
    while rest.len() >= 8 && !(in_frame && chunks == 2) {
        chunks += 1;
        let size = usize::try_from(u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]))
            .map_err(|_| Stop::Unparsed)?;
        let body = rest
            .get(8..)
            .map(|r| &r[..size.min(r.len())])
            .ok_or(Stop::Unparsed)?;
        match &rest[..4] {
            b"VP8L" if !seen_lossless => {
                seen_lossless = true;
                check_lossless_chunk(body)?;
            }
            b"ALPH" if !seen_alpha => {
                seen_alpha = true;
                alpha = Some(body);
            }
            b"VP8 " if !seen_lossy => {
                seen_lossy = true;
                frame_size = vp8_size(body);
            }
            b"ANMF" if !in_frame && !seen_frame => {
                seen_frame = true;
                if let Some(frame) = body.get(16..) {
                    check_chunks(frame, true)?;
                }
            }
            _ => {}
        }
        let step = 8usize
            .checked_add(size)
            .and_then(|n| n.checked_add(size & 1))
            .ok_or(Stop::Unparsed)?;
        rest = rest.get(step..).unwrap_or(&[]);
    }
    if let (Some(alpha), Some((w, h))) = (alpha, frame_size) {
        check_alpha(alpha, w, h)?;
    }
    Ok(())
}

/// The size a lossy frame declares: 14 bits each after the keyframe start code.
fn vp8_size(body: &[u8]) -> Option<(usize, usize)> {
    if body.get(3..6) != Some(&[0x9d, 0x01, 0x2a]) {
        return None;
    }
    let w = usize::from(u16::from_le_bytes([*body.get(6)?, *body.get(7)?]) & 0x3fff);
    let h = usize::from(u16::from_le_bytes([*body.get(8)?, *body.get(9)?]) & 0x3fff);
    Some((w, h))
}

/// A `VP8L` chunk: a signature, 14-bit sizes, then the prefix-coded stream.
fn check_lossless_chunk(body: &[u8]) -> Walk<()> {
    if body.first() != Some(&0x2f) {
        return Err(Stop::Unparsed);
    }
    let h = body.get(1..5).ok_or(Stop::Unparsed)?;
    let header = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
    if header >> 29 != 0 {
        return Err(Stop::Unparsed);
    }
    let width = usize::try_from((1 + header) & 0x3fff).map_err(|_| Stop::Unparsed)?;
    let height = usize::try_from((1 + (header >> 14)) & 0x3fff).map_err(|_| Stop::Unparsed)?;
    walk_image(&mut Bits::new(&body[5..]), width, height)
}

/// An `ALPH` chunk with lossless compression: one info byte, then a stream
/// with no header, sized like the frame.
fn check_alpha(body: &[u8], width: usize, height: usize) -> Walk<()> {
    match body.first() {
        Some(info) if info & 0b11 == 1 && width > 0 && height > 0 => {
            walk_image(&mut Bits::new(&body[1..]), width, height)
        }
        _ => Ok(()),
    }
}

/// Reads the transforms and the headers of the main image, checking every
/// prefix code on the way. The pixel data of the main image is not read.
fn walk_image(bits: &mut Bits, width: usize, height: usize) -> Walk<()> {
    let mut xsize = width;
    let mut seen = [false; 4];
    while bits.read(1)? == 1 {
        let kind = bits.read(2)? as usize;
        if std::mem::replace(&mut seen[kind], true) {
            return Err(Stop::Unparsed);
        }
        match kind {
            0 | 1 => {
                let size_bits = bits.read(3)? + 2;
                let (bw, bh) = (subsample(xsize, size_bits), subsample(height, size_bits));
                read_sub_image(bits, bw, bh)?;
            }
            2 => {}
            _ => {
                let table = bits.read(8)? as usize + 1;
                read_sub_image(bits, table, 1)?;
                let pack = match table {
                    0..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                xsize = subsample(xsize, pack);
            }
        }
    }
    let cache_bits = read_color_cache(bits)?;
    read_codes(bits, true, xsize, height, cache_bits)?;
    Ok(())
}

fn subsample(size: usize, bits: u32) -> usize {
    (size + (1usize << bits) - 1) >> bits
}

fn read_color_cache(bits: &mut Bits) -> Walk<Option<u32>> {
    if bits.read(1)? == 1 {
        let n = bits.read(4)?;
        if !(1..=11).contains(&n) {
            return Err(Stop::Unparsed);
        }
        Ok(Some(n))
    } else {
        Ok(None)
    }
}

/// An entropy-coded sub-image: read it fully, since where the stream goes next
/// depends on its pixels.
fn read_sub_image(bits: &mut Bits, width: usize, height: usize) -> Walk<()> {
    sub_image(bits, width, height).map(|_| ())
}

fn sub_image(bits: &mut Bits, width: usize, height: usize) -> Walk<Vec<u32>> {
    let total = width.checked_mul(height).ok_or(Stop::Unparsed)?;
    if total == 0 || total > MAX_SUB_IMAGE_PIXELS {
        return Err(Stop::Unparsed);
    }
    let cache_bits = read_color_cache(bits)?;
    let mut groups = read_codes(bits, false, width, height, cache_bits)?;
    let group = groups.pop().ok_or(Stop::Unparsed)?;
    let mut cache = cache_bits.map(|n| vec![0u32; 1usize << n]);

    let mut data = vec![0u32; total];
    let mut index = 0;
    while index < total {
        let symbol = group[0].read(bits)?;
        let code = usize::from(symbol);
        if code < 256 {
            let red = group[1].read(bits)?;
            let blue = group[2].read(bits)?;
            let alpha = group[3].read(bits)?;
            let px = u32::from(alpha) << 24
                | u32::from(red) << 16
                | u32::from(symbol) << 8
                | u32::from(blue);
            data[index] = px;
            insert(&mut cache, cache_bits, px);
            index += 1;
        } else if code < 256 + 24 {
            let length = copy_distance(bits, code - 256)?;
            let dist_symbol = group[4].read(bits)?;
            let dist_code = copy_distance(bits, usize::from(dist_symbol))?;
            let dist = plane_code_to_distance(width, dist_code);
            if index < dist || total - index < length {
                return Err(Stop::Unparsed);
            }
            for i in 0..length {
                let px = data[index + i - dist];
                data[index + i] = px;
                insert(&mut cache, cache_bits, px);
            }
            index += length;
        } else {
            let cache = cache.as_ref().ok_or(Stop::Unparsed)?;
            data[index] = *cache.get(code - 280).ok_or(Stop::Unparsed)?;
            index += 1;
        }
    }
    Ok(data)
}

fn insert(cache: &mut Option<Vec<u32>>, cache_bits: Option<u32>, px: u32) {
    if let (Some(cache), Some(n)) = (cache.as_mut(), cache_bits) {
        let slot = (0x1e35_a7bdu32.wrapping_mul(px)) >> (32 - n);
        if let Some(entry) = cache.get_mut(slot as usize) {
            *entry = px;
        }
    }
}

fn copy_distance(bits: &mut Bits, prefix: usize) -> Walk<usize> {
    if prefix < 4 {
        return Ok(prefix + 1);
    }
    let extra = u32::try_from((prefix - 2) >> 1).map_err(|_| Stop::Unparsed)?;
    let offset = (2 + (prefix & 1)) << extra;
    Ok(offset + bits.read(extra)? as usize + 1)
}

fn plane_code_to_distance(width: usize, code: usize) -> usize {
    if code > 120 {
        return code - 120;
    }
    let (dx, dy) = DISTANCE_MAP[code - 1];
    let dist = i64::from(dx) + i64::from(dy) * i64::try_from(width).unwrap_or(i64::MAX / 16);
    usize::try_from(dist).map_or(1, |d| d.max(1))
}

/// Reads the prefix-code groups of an image. The main image may have an entropy
/// image choosing among many groups; only the last group is kept for a
/// sub-image, which has exactly one.
fn read_codes(
    bits: &mut Bits,
    meta: bool,
    xsize: usize,
    ysize: usize,
    cache_bits: Option<u32>,
) -> Walk<Vec<[Code; 5]>> {
    let mut groups = 1usize;
    if meta && bits.read(1)? == 1 {
        let size_bits = bits.read(3)? + 2;
        let image = sub_image(
            bits,
            subsample(xsize, size_bits),
            subsample(ysize, size_bits),
        )?;
        groups = image
            .iter()
            .map(|px| ((px >> 16) & 0xff) << 8 | ((px >> 8) & 0xff))
            .max()
            .map_or(1, |m| m as usize + 1);
    }
    let mut kept = Vec::new();
    for _ in 0..groups {
        let mut group: [Code; 5] = Default::default();
        for (j, slot) in group.iter_mut().enumerate() {
            let mut size = ALPHABET_SIZE[j];
            if j == 0 {
                size += cache_bits.map_or(0, |n| 1usize << n);
            }
            *slot = read_code(bits, size)?;
        }
        if !meta {
            kept.push(group);
        }
    }
    Ok(kept)
}

/// One prefix code: a single symbol, two symbols, or canonical by length.
#[derive(Default)]
enum Code {
    #[default]
    Single0,
    Single(u16),
    Two(u16, u16),
    Canonical {
        count: [u16; MAX_CODE_LENGTH + 1],
        symbols: Vec<u16>,
    },
}

impl Code {
    fn read(&self, bits: &mut Bits) -> Walk<u16> {
        match self {
            Code::Single0 => Ok(0),
            Code::Single(s) => Ok(*s),
            Code::Two(zero, one) => Ok(if bits.read(1)? == 1 { *one } else { *zero }),
            Code::Canonical { count, symbols } => {
                let (mut code, mut first, mut index) = (0usize, 0usize, 0usize);
                for &n in &count[1..] {
                    code |= bits.read(1)? as usize;
                    let n = usize::from(n);
                    if code < first + n {
                        return symbols
                            .get(index + code - first)
                            .copied()
                            .ok_or(Stop::Unparsed);
                    }
                    index += n;
                    first = (first + n) << 1;
                    code <<= 1;
                }
                Err(Stop::Unparsed)
            }
        }
    }
}

/// Builds the code the library would build from `lengths`, or [`Stop::BadCode`]
/// where the library returns an error: no symbol, or lengths that do not fill
/// the code exactly. The sums here are wide, so they cannot overflow.
fn from_lengths(lengths: &[u16]) -> Walk<Code> {
    let mut count = [0u16; MAX_CODE_LENGTH + 1];
    let mut used = 0usize;
    for &len in lengths.iter().filter(|&&l| l != 0) {
        *count.get_mut(usize::from(len)).ok_or(Stop::Unparsed)? += 1;
        used += 1;
    }
    match used {
        0 => return Err(Stop::BadCode),
        1 => {
            let sym = lengths.iter().position(|&l| l != 0).unwrap_or(0);
            return Ok(Code::Single(
                u16::try_from(sym).map_err(|_| Stop::Unparsed)?,
            ));
        }
        _ => {}
    }
    let mut left: i64 = 1;
    for &n in &count[1..] {
        left = (left << 1) - i64::from(n);
        if left < 0 {
            return Err(Stop::BadCode);
        }
    }
    if left != 0 {
        return Err(Stop::BadCode);
    }
    let mut symbols = Vec::with_capacity(used);
    for len in 1..=MAX_CODE_LENGTH {
        for (sym, &l) in lengths.iter().enumerate() {
            if usize::from(l) == len {
                symbols.push(u16::try_from(sym).map_err(|_| Stop::Unparsed)?);
            }
        }
    }
    Ok(Code::Canonical { count, symbols })
}

fn read_code(bits: &mut Bits, alphabet: usize) -> Walk<Code> {
    if bits.read(1)? == 1 {
        let two = bits.read(1)? == 1;
        let first_8 = bits.read(1)?;
        let zero = bits.read(1 + 7 * first_8)? as usize;
        if zero >= alphabet {
            return Err(Stop::Unparsed);
        }
        let zero = u16::try_from(zero).map_err(|_| Stop::Unparsed)?;
        if !two {
            return Ok(Code::Single(zero));
        }
        let one = bits.read(8)? as usize;
        if one >= alphabet {
            return Err(Stop::Unparsed);
        }
        return Ok(Code::Two(
            zero,
            u16::try_from(one).map_err(|_| Stop::Unparsed)?,
        ));
    }

    let mut cl_lengths = [0u16; CODE_LENGTH_CODES];
    let n = 4 + bits.read(4)? as usize;
    for &slot in CODE_LENGTH_CODE_ORDER.iter().take(n) {
        cl_lengths[slot] = u16::try_from(bits.read(3)?).map_err(|_| Stop::Unparsed)?;
    }
    let table = from_lengths(&cl_lengths)?;

    let mut max_symbol = if bits.read(1)? == 1 {
        let nbits = 2 + 2 * bits.read(3)?;
        let max_minus_two = bits.read(nbits)? as usize;
        if max_minus_two > alphabet - 2 {
            return Err(Stop::Unparsed);
        }
        2 + max_minus_two
    } else {
        alphabet
    };

    let mut lengths = vec![0u16; alphabet];
    let mut prev = 8u16;
    let mut symbol = 0usize;
    while symbol < alphabet && max_symbol > 0 {
        max_symbol -= 1;
        let len = table.read(bits)?;
        if len < 16 {
            lengths[symbol] = len;
            symbol += 1;
            if len != 0 {
                prev = len;
            }
        } else {
            let (extra, offset) = match len {
                16 => (2, 3),
                17 => (3, 3),
                18 => (7, 11),
                _ => return Err(Stop::Unparsed),
            };
            let repeat = bits.read(extra)? as usize + offset;
            if symbol + repeat > alphabet {
                return Err(Stop::Unparsed);
            }
            let value = if len == 16 { prev } else { 0 };
            lengths[symbol..symbol + repeat].fill(value);
            symbol += repeat;
        }
    }
    from_lengths(&lengths)
}

/// A little-endian bit reader over a byte slice.
struct Bits<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// The next `n` (at most 32) bits, first bit lowest. Past the end is
    /// [`Stop::Unparsed`].
    fn read(&mut self, n: u32) -> Walk<u32> {
        if n > 32 || self.pos + n as usize > self.bytes.len() * 8 {
            return Err(Stop::Unparsed);
        }
        let mut value = 0u64;
        for i in 0..n {
            let at = self.pos + i as usize;
            let bit = (self.bytes[at / 8] >> (at % 8)) & 1;
            value |= u64::from(bit) << i;
        }
        self.pos += n as usize;
        u32::try_from(value).map_err(|_| Stop::Unparsed)
    }
}
