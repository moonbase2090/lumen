//! The decoder against images whose pixels are known. The mutation sweep and the
//! regression seeds show that damaged and odd files are answered cleanly; this
//! shows that undamaged files come out right: every pixel of every PNG colour
//! type and bit depth, with and without a transparency key or palette alpha,
//! with every row filter and with Adam7 interlacing (including images so small
//! that some interlace passes are empty); lossless WebP bit for bit; JPEG in
//! the layouts encoders produce, within the error a lossy codec is allowed.
//!
//! The PNG files are written here, by a small writer that knows nothing of the
//! decoder's code (stored zlib blocks, hand-packed samples, hand-applied
//! filters and passes), so the expected pixels come from the samples that went
//! in and not from a second reading of the decoder's own library.

#![allow(missing_docs)]

use std::error::Error;

use lumen_decoder::decode::{Decoded, decode};
use lumen_decoder::protocol::{Format, Status};

type R = Result<(), Box<dyn Error>>;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u32) -> u32 {
        u32::try_from(self.next() % u64::from(n.max(1))).unwrap_or(0)
    }
}

/// Premultiplication as the decoder's contract states it: rounded, and only
/// when the alpha is not opaque.
fn premultiplied(px: [u8; 4]) -> [u8; 4] {
    let a = u32::from(px[3]);
    if a == 255 {
        return px;
    }
    let p = |c: u8| u8::try_from((u32::from(c) * a + 127) / 255).unwrap_or(255);
    [p(px[0]), p(px[1]), p(px[2]), px[3]]
}

fn flat(px: &[[u8; 4]]) -> Vec<u8> {
    px.iter().flat_map(|p| premultiplied(*p)).collect()
}

fn decoded(format: Format, bytes: &[u8]) -> Result<Decoded, Status> {
    let d = decode(format, bytes)?;
    assert_eq!(
        d.rgba.len(),
        d.width as usize * d.height as usize * 4,
        "the decode contract: four bytes a pixel"
    );
    Ok(d)
}

// ---- a small PNG writer ---------------------------------------------------

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

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], body: &[u8]) {
    out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    let mut covered = kind.to_vec();
    covered.extend_from_slice(body);
    out.extend_from_slice(&covered);
    out.extend_from_slice(&crc32(&covered).to_be_bytes());
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        let len = u16::try_from(block.len()).unwrap_or(0);
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Gray,
    Rgb,
    Indexed,
    GrayAlpha,
    Rgba,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Kind::Gray => 0,
            Kind::Rgb => 2,
            Kind::Indexed => 3,
            Kind::GrayAlpha => 4,
            Kind::Rgba => 6,
        }
    }
    fn channels(self) -> usize {
        match self {
            Kind::Gray | Kind::Indexed => 1,
            Kind::GrayAlpha => 2,
            Kind::Rgb => 3,
            Kind::Rgba => 4,
        }
    }
}

struct Spec {
    kind: Kind,
    depth: u8,
    /// A transparency key (gray, RGB) or alpha for the first palette entries.
    trns: bool,
    interlaced: bool,
    w: u32,
    h: u32,
}

impl Spec {
    fn name(&self) -> String {
        format!(
            "{:?} depth {} trns {} interlaced {} {}x{}",
            self.kind, self.depth, self.trns, self.interlaced, self.w, self.h
        )
    }
}

/// Packs one row of samples (each `depth` bits) into bytes.
fn pack(samples: &[u16], depth: u8) -> Vec<u8> {
    let mut row = Vec::new();
    match depth {
        16 => {
            for s in samples {
                row.extend_from_slice(&s.to_be_bytes());
            }
        }
        8 => row.extend(samples.iter().map(|s| u8::try_from(*s).unwrap_or(0))),
        _ => {
            let per = usize::from(8 / depth);
            for group in samples.chunks(per) {
                let mut byte = 0u8;
                for (i, s) in group.iter().enumerate() {
                    let shift = 8 - usize::from(depth) * (i + 1);
                    byte |= u8::try_from(*s).unwrap_or(0) << shift;
                }
                row.push(byte);
            }
        }
    }
    row
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filters `row` with filter type `ft` given the previous unfiltered row.
fn filter(ft: u8, row: &[u8], prev: &[u8], bpp: usize) -> Vec<u8> {
    let mut out = vec![ft];
    for i in 0..row.len() {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let predictor = match ft {
            1 => a,
            2 => b,
            3 => u8::midpoint(a, b),
            4 => paeth(a, b, c),
            _ => 0,
        };
        out.push(row[i].wrapping_sub(predictor));
    }
    out
}

const PASSES: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

fn pass_size(w: u32, h: u32, (x0, y0, dx, dy): (u32, u32, u32, u32)) -> (u32, u32) {
    let n = |len: u32, start: u32, step: u32| {
        if len > start {
            (len - start).div_ceil(step)
        } else {
            0
        }
    };
    (n(w, x0, dx), n(h, y0, dy))
}

/// A PNG file for `spec` with random samples, and the premultiplied RGBA the
/// decoder has to return for it.
#[allow(clippy::too_many_lines)] // one pass over a spec: samples, expected pixels, file
fn png_with_pixels(spec: &Spec, rng: &mut Rng) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (spec.w as usize, spec.h as usize);
    let max = if spec.depth == 16 {
        65535u32
    } else {
        (1u32 << spec.depth) - 1
    };
    let channels = spec.kind.channels();
    let sample = |rng: &mut Rng| u16::try_from(rng.below(max + 1)).unwrap_or(0);
    // Palette: as many entries as the depth allows, at most 256.
    let entries = if spec.kind == Kind::Indexed {
        usize::try_from(max + 1).unwrap_or(256).min(256)
    } else {
        0
    };
    let palette: Vec<[u8; 3]> = (0..entries)
        .map(|_| {
            [
                u8::try_from(rng.below(256)).unwrap_or(0),
                u8::try_from(rng.below(256)).unwrap_or(0),
                u8::try_from(rng.below(256)).unwrap_or(0),
            ]
        })
        .collect();
    // Alpha for the first palette entries, so the rest stay opaque.
    let alphas: Vec<u8> = if spec.trns && spec.kind == Kind::Indexed {
        (0..entries.div_ceil(2))
            .map(|_| u8::try_from(rng.below(256)).unwrap_or(0))
            .collect()
    } else {
        Vec::new()
    };
    // The transparent colour for gray and RGB.
    let key: Vec<u16> = (0..channels).map(|_| sample(rng)).collect();

    // Samples of every pixel, in image order.
    let mut samples: Vec<Vec<u16>> = Vec::with_capacity(w * h);
    for _ in 0..w * h {
        let mut px: Vec<u16> = (0..channels)
            .map(|_| {
                if spec.kind == Kind::Indexed {
                    u16::try_from(rng.below(u32::try_from(entries).unwrap_or(1))).unwrap_or(0)
                } else {
                    sample(rng)
                }
            })
            .collect();
        // Make the key common so the transparent pixels are tested.
        if spec.trns && matches!(spec.kind, Kind::Gray | Kind::Rgb) && rng.below(3) == 0 {
            px.clone_from(&key);
        }
        samples.push(px);
    }

    // The expected pixels.
    let to8 = |v: u16| -> u8 {
        match spec.depth {
            16 => u8::try_from(v >> 8).unwrap_or(0),
            8 => u8::try_from(v).unwrap_or(0),
            d => u8::try_from(u32::from(v) * 255 / ((1u32 << d) - 1)).unwrap_or(0),
        }
    };
    let expected: Vec<[u8; 4]> = samples
        .iter()
        .map(|px| match spec.kind {
            Kind::Gray => {
                let a = if spec.trns && px[0] == key[0] { 0 } else { 255 };
                [to8(px[0]), to8(px[0]), to8(px[0]), a]
            }
            Kind::Rgb => {
                let a = if spec.trns && px == &key { 0 } else { 255 };
                [to8(px[0]), to8(px[1]), to8(px[2]), a]
            }
            Kind::Indexed => {
                let i = usize::from(px[0]);
                let a = alphas.get(i).copied().unwrap_or(255);
                [palette[i][0], palette[i][1], palette[i][2], a]
            }
            Kind::GrayAlpha => [to8(px[0]), to8(px[0]), to8(px[0]), to8(px[1])],
            Kind::Rgba => [to8(px[0]), to8(px[1]), to8(px[2]), to8(px[3])],
        })
        .collect();

    // The scanlines, with a random filter on every row.
    let bits = usize::from(spec.depth) * channels;
    let bpp = bits.div_ceil(8);
    let mut raw = Vec::new();
    // Filtering needs the previous row of the same pass, so keep it per pass.
    let write_pass = |xs: Vec<usize>, ys: Vec<usize>, rng: &mut Rng, raw: &mut Vec<u8>| {
        let mut prev: Vec<u8> = Vec::new();
        for &y in &ys {
            let row: Vec<u16> = xs
                .iter()
                .flat_map(|&x| samples[y * w + x].clone())
                .collect();
            let packed = pack(&row, spec.depth);
            if prev.len() != packed.len() {
                prev = vec![0u8; packed.len()];
            }
            let ft = u8::try_from(rng.below(5)).unwrap_or(0);
            raw.extend(filter(ft, &packed, &prev, bpp));
            prev = packed;
        }
    };
    if spec.interlaced {
        for pass in PASSES {
            let (pw, ph) = pass_size(spec.w, spec.h, pass);
            if pw == 0 || ph == 0 {
                continue;
            }
            let (x0, y0, dx, dy) = (
                pass.0 as usize,
                pass.1 as usize,
                pass.2 as usize,
                pass.3 as usize,
            );
            let xs = (0..pw as usize).map(|i| x0 + i * dx).collect();
            let ys = (0..ph as usize).map(|i| y0 + i * dy).collect();
            write_pass(xs, ys, rng, &mut raw);
        }
    } else {
        write_pass((0..w).collect(), (0..h).collect(), rng, &mut raw);
    }

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&spec.w.to_be_bytes());
    ihdr.extend_from_slice(&spec.h.to_be_bytes());
    ihdr.extend_from_slice(&[
        spec.depth,
        spec.kind.code(),
        0,
        0,
        u8::from(spec.interlaced),
    ]);
    chunk(&mut out, *b"IHDR", &ihdr);
    if spec.kind == Kind::Indexed {
        chunk(&mut out, *b"PLTE", &palette.concat());
        if spec.trns {
            chunk(&mut out, *b"tRNS", &alphas);
        }
    } else if spec.trns {
        let mut body = Vec::new();
        for k in &key {
            body.extend_from_slice(&k.to_be_bytes());
        }
        chunk(&mut out, *b"tRNS", &body);
    }
    chunk(&mut out, *b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, *b"IEND", &[]);
    (out, flat(&expected))
}

fn png_specs() -> Vec<Spec> {
    let combos: Vec<(Kind, u8)> = vec![
        (Kind::Gray, 1),
        (Kind::Gray, 2),
        (Kind::Gray, 4),
        (Kind::Gray, 8),
        (Kind::Gray, 16),
        (Kind::Rgb, 8),
        (Kind::Rgb, 16),
        (Kind::Indexed, 1),
        (Kind::Indexed, 2),
        (Kind::Indexed, 4),
        (Kind::Indexed, 8),
        (Kind::GrayAlpha, 8),
        (Kind::GrayAlpha, 16),
        (Kind::Rgba, 8),
        (Kind::Rgba, 16),
    ];
    // Sizes: one pixel, one row, one column, rows that are not a whole number
    // of bytes, and sizes where some interlace passes are empty.
    let sizes = [(1, 1), (1, 9), (9, 1), (3, 5), (8, 8), (13, 7), (17, 33)];
    let mut out = Vec::new();
    for (kind, depth) in combos {
        for trns in [false, true] {
            if trns && matches!(kind, Kind::GrayAlpha | Kind::Rgba) {
                continue;
            }
            for interlaced in [false, true] {
                for (w, h) in sizes {
                    out.push(Spec {
                        kind,
                        depth,
                        trns,
                        interlaced,
                        w,
                        h,
                    });
                }
            }
        }
    }
    out
}

#[test]
fn every_png_colour_type_depth_filter_and_interlace_gives_the_pixels_that_went_in() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut checked = 0;
    let mut bad = Vec::new();
    for spec in png_specs() {
        // A few random images per layout, so each filter type meets each layout.
        for _ in 0..3 {
            let (file, want) = png_with_pixels(&spec, &mut rng);
            match decoded(Format::Png, &file) {
                Ok(d) if (d.width, d.height) == (spec.w, spec.h) && d.rgba == want => {}
                Ok(d) => bad.push(format!(
                    "{}: {}x{}, {} of {} bytes differ",
                    spec.name(),
                    d.width,
                    d.height,
                    d.rgba.iter().zip(&want).filter(|(a, b)| a != b).count(),
                    want.len()
                )),
                Err(status) => bad.push(format!("{}: refused as {status:?}", spec.name())),
            }
            checked += 1;
        }
    }
    assert!(checked > 600, "the matrix shrank: {checked}");
    assert!(
        bad.is_empty(),
        "{} of {checked} wrong:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

// ---- WebP -----------------------------------------------------------------

fn noisy_rgba(w: u32, h: u32, rng: &mut Rng, alpha: bool) -> Vec<[u8; 4]> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let band = (x / 4 + y / 3) % 3;
            let px = match band {
                // A flat run, so run-length and colour-cache coding is used.
                0 => [200, 30, 90, 255],
                // A smooth ramp.
                1 => [
                    u8::try_from((x * 9) % 256).unwrap_or(0),
                    u8::try_from((y * 11) % 256).unwrap_or(0),
                    u8::try_from((x + y) % 256).unwrap_or(0),
                    255,
                ],
                _ => [
                    u8::try_from(rng.below(256)).unwrap_or(0),
                    u8::try_from(rng.below(256)).unwrap_or(0),
                    u8::try_from(rng.below(256)).unwrap_or(0),
                    255,
                ],
            };
            let a = if alpha {
                u8::try_from(rng.below(256)).unwrap_or(255)
            } else {
                255
            };
            v.push([px[0], px[1], px[2], a]);
        }
    }
    v
}

#[test]
fn lossless_webp_comes_back_bit_for_bit() -> R {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for (w, h) in [(1u32, 1u32), (1, 17), (17, 1), (16, 16), (33, 19), (97, 61)] {
        for alpha in [false, true] {
            let px = noisy_rgba(w, h, &mut rng, alpha);
            let mut file = Vec::new();
            let (data, color): (Vec<u8>, _) = if alpha {
                (
                    px.iter().flatten().copied().collect(),
                    image_webp::ColorType::Rgba8,
                )
            } else {
                (
                    px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
                    image_webp::ColorType::Rgb8,
                )
            };
            image_webp::WebPEncoder::new(&mut file).encode(&data, w, h, color)?;
            let d = decoded(Format::Webp, &file).map_err(|s| format!("{w}x{h} {alpha}: {s:?}"))?;
            assert_eq!((d.width, d.height), (w, h));
            assert!(
                d.rgba == flat(&px),
                "{w}x{h} alpha {alpha}: the pixels differ"
            );
        }
    }
    Ok(())
}

// ---- JPEG -----------------------------------------------------------------

/// A smooth image, because a lossy codec is only expected to be close on one.
fn smooth(w: u32, h: u32) -> Vec<[u8; 3]> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.push([
                u8::try_from(40 + x * 150 / w.max(1)).unwrap_or(255),
                u8::try_from(200 - y * 150 / h.max(1)).unwrap_or(0),
                u8::try_from(60 + (x + y) * 100 / (w + h).max(1)).unwrap_or(0),
            ]);
        }
    }
    v
}

struct JpegMode {
    name: &'static str,
    sampling: jpeg_encoder::SamplingFactor,
    progressive: bool,
    optimized: bool,
    restart: u16,
}

fn jpeg_modes() -> Vec<JpegMode> {
    use jpeg_encoder::SamplingFactor as S;
    let mut v = Vec::new();
    for (name, sampling) in [
        ("4:4:4", S::F_1_1),
        ("4:2:2", S::F_2_1),
        ("4:2:0", S::F_2_2),
        ("4:4:0", S::F_1_2),
    ] {
        for (progressive, optimized, restart) in [
            (false, false, 0),
            (true, false, 0),
            (false, true, 0),
            (false, false, 3),
        ] {
            v.push(JpegMode {
                name,
                sampling,
                progressive,
                optimized,
                restart,
            });
        }
    }
    v
}

#[test]
fn a_jpeg_in_every_layout_is_close_to_the_image_that_went_in() -> R {
    let mut bad = Vec::new();
    for (w, h) in [(1u16, 1u16), (7, 5), (16, 16), (33, 17), (64, 48)] {
        let px = smooth(u32::from(w), u32::from(h));
        let rgb: Vec<u8> = px.iter().flatten().copied().collect();
        for mode in jpeg_modes() {
            let mut file = Vec::new();
            let mut enc = jpeg_encoder::Encoder::new(&mut file, 100);
            enc.set_sampling_factor(mode.sampling);
            enc.set_progressive(mode.progressive);
            enc.set_optimized_huffman_tables(mode.optimized);
            enc.set_restart_interval(mode.restart);
            enc.encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)?;
            let label = format!(
                "{w}x{h} {} progressive {} optimized {} restart {}",
                mode.name, mode.progressive, mode.optimized, mode.restart
            );
            let d = match decoded(Format::Jpeg, &file) {
                Ok(d) => d,
                Err(s) => {
                    bad.push(format!("{label}: refused as {s:?}"));
                    continue;
                }
            };
            if (d.width, d.height) != (u32::from(w), u32::from(h)) {
                bad.push(format!("{label}: came out {}x{}", d.width, d.height));
                continue;
            }
            // Chroma subsampling blurs colour across blocks, so a corner pixel
            // can be off by a good deal more than at 4:4:4; the bound is for
            // "the right picture", not for exactness.
            let worst = d
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(&px)
                .flat_map(|(got, want)| {
                    (0..3)
                        .map(move |c| u8::abs_diff(got[c], want[c]))
                        .chain(std::iter::once(255 - got[3]))
                })
                .max()
                .unwrap_or(0);
            if worst > 40 {
                bad.push(format!("{label}: worst channel error {worst}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
    Ok(())
}

#[test]
fn a_gray_jpeg_is_gray_and_opaque() -> R {
    let (w, h) = (24u16, 20u16);
    let luma: Vec<u8> = (0..u32::from(w) * u32::from(h))
        .map(|i| u8::try_from(i * 255 / (u32::from(w) * u32::from(h))).unwrap_or(0))
        .collect();
    let mut file = Vec::new();
    jpeg_encoder::Encoder::new(&mut file, 100).encode(
        &luma,
        w,
        h,
        jpeg_encoder::ColorType::Luma,
    )?;
    let d = decoded(Format::Jpeg, &file).map_err(|s| format!("{s:?}"))?;
    for (i, (got, &want)) in d.rgba.as_chunks::<4>().0.iter().zip(&luma).enumerate() {
        assert_eq!(got[3], 255, "pixel {i} is opaque");
        assert!(
            got[..3].iter().all(|&c| u8::abs_diff(c, want) <= 6),
            "pixel {i}: {got:?} vs gray {want}"
        );
    }
    Ok(())
}

#[test]
fn a_four_channel_jpeg_is_decoded_or_refused_but_never_the_wrong_size() -> R {
    // CMYK and YCCK files exist in the wild (Adobe, print workflows). Either
    // answer is acceptable to the contract; a wrong size or a panic is not.
    // Their colours are not checked here: with the same ink values, CMYK gives
    // the expected colours and YCCK comes out dark, and no reference decoder
    // is at hand to say which of the encoder and the decoder is at fault.
    let (w, h) = (16u16, 16u16);
    let data: Vec<u8> = (0..usize::from(w) * usize::from(h) * 4)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect();
    for color in [jpeg_encoder::ColorType::Cmyk, jpeg_encoder::ColorType::Ycck] {
        let mut file = Vec::new();
        jpeg_encoder::Encoder::new(&mut file, 90).encode(&data, w, h, color)?;
        // `decoded` checks the contract on a success; a refusal is fine.
        let _ = decoded(Format::Jpeg, &file);
    }
    Ok(())
}
