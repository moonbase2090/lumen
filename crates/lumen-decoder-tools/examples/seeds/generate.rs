//! Seed images made by real encoders. Past-CVE inputs are not generated here:
//! they have to be collected from public reproducers (see the campaign notes).

use std::error::Error;

type R<T> = Result<T, Box<dyn Error>>;

fn gradient(w: u32, h: u32, channels: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in 0..channels {
                v.push(u8::try_from((x * 7 + y * 13 + c * 61) % 256).unwrap_or(0));
            }
        }
    }
    v
}

fn png(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, channels: u32) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(color);
    enc.set_depth(depth);
    if color == png::ColorType::Indexed {
        enc.set_palette(
            (0..=255u8)
                .flat_map(|i| [i, 255 - i, i / 2])
                .collect::<Vec<u8>>(),
        );
    }
    let mut wr = enc.write_header()?;
    let bytes_per = if depth == png::BitDepth::Sixteen {
        2
    } else {
        1
    };
    let mut data = gradient(w, h, channels * bytes_per);
    if matches!(
        depth,
        png::BitDepth::One | png::BitDepth::Two | png::BitDepth::Four
    ) {
        let bits = depth as usize;
        let row = (w as usize * channels as usize * bits).div_ceil(8);
        data = (0..row * h as usize)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
    }
    wr.write_image_data(&data)?;
    drop(wr);
    Ok(out)
}

/// Named seeds for each format: `(file name, bytes)`.
///
/// # Errors
/// If an encoder fails.
pub fn all() -> R<Vec<(String, Vec<u8>)>> {
    use png::{BitDepth as D, ColorType as C};
    let mut v = Vec::new();
    for (name, color, depth, ch) in [
        ("rgba8", C::Rgba, D::Eight, 4),
        ("rgb8", C::Rgb, D::Eight, 3),
        ("gray8", C::Grayscale, D::Eight, 1),
        ("graya8", C::GrayscaleAlpha, D::Eight, 2),
        ("rgba16", C::Rgba, D::Sixteen, 4),
        ("gray1", C::Grayscale, D::One, 1),
        ("gray4", C::Grayscale, D::Four, 1),
        ("indexed8", C::Indexed, D::Eight, 1),
    ] {
        v.push((format!("png/{name}.png"), png(17, 9, color, depth, ch)?));
    }
    v.push(("png/one-pixel.png".into(), png(1, 1, C::Rgba, D::Eight, 4)?));
    for (name, w, h, q) in [
        ("small", 8u16, 8u16, 90u8),
        ("odd", 33, 17, 50),
        ("wide", 200, 3, 100),
    ] {
        let mut out = Vec::new();
        let enc = jpeg_encoder::Encoder::new(&mut out, q);
        enc.encode(
            &gradient(u32::from(w), u32::from(h), 4),
            w,
            h,
            jpeg_encoder::ColorType::Rgba,
        )?;
        v.push((format!("jpeg/{name}.jpg"), out));
    }
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, 80);
    enc.encode(&gradient(16, 16, 1), 16, 16, jpeg_encoder::ColorType::Luma)?;
    v.push(("jpeg/gray.jpg".into(), out));
    for (name, w, h, rgba) in [
        ("rgba", 19u32, 11u32, true),
        ("rgb", 12, 12, false),
        ("one", 1, 1, true),
    ] {
        let mut out = Vec::new();
        let (ch, ct) = if rgba {
            (4, image_webp::ColorType::Rgba8)
        } else {
            (3, image_webp::ColorType::Rgb8)
        };
        image_webp::WebPEncoder::new(&mut out).encode(&gradient(w, h, ch), w, h, ct)?;
        v.push((format!("webp/{name}.webp"), out));
    }
    Ok(v)
}
