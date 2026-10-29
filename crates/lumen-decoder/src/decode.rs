//! Decoding of the first frame to 8-bit premultiplied, top-down RGBA.
//!
//! Every size is checked against the limits on the decoded header, in 64-bit
//! arithmetic, before the output buffer is allocated. The hint in the request
//! is never used here.

use std::io::Cursor;

use crate::limits::{self, MAX_PIXELS, MAX_SIDE};
use crate::protocol::{Format, Status};

/// A decoded image.
#[derive(Debug)]
pub struct Decoded {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of premultiplied RGBA.
    pub rgba: Vec<u8>,
}

/// Decodes the first frame of `bytes`, which must be of `format`.
///
/// # Errors
/// `Unsupported` if the signature does not match `format`, `TooLarge` over a
/// size limit, `Corrupt` if the library rejects the data.
pub fn decode(format: Format, bytes: &[u8]) -> Result<Decoded, Status> {
    if Format::sniff(bytes) != Some(format) {
        return Err(Status::Unsupported);
    }
    let mut d = match format {
        Format::Png => png(bytes)?,
        Format::Jpeg => jpeg(bytes)?,
        Format::Webp => webp(bytes)?,
    };
    premultiply(&mut d.rgba);
    Ok(d)
}

fn check_dims(width: u64, height: u64) -> Result<usize, Status> {
    let bytes = limits::rgba_bytes(width, height).ok_or(Status::TooLarge)?;
    usize::try_from(bytes).map_err(|_| Status::TooLarge)
}

fn dim32(v: u64) -> Result<u32, Status> {
    u32::try_from(v).map_err(|_| Status::TooLarge)
}

fn premultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a != 255 {
            for c in &mut px[..3] {
                *c = u8::try_from((u32::from(*c) * a + 127) / 255).unwrap_or(255);
            }
        }
    }
}

/// Whether every `PLTE` chunk has a length that is a multiple of 3, as the PNG
/// specification requires.
///
/// Workaround for `png` 0.18.1, which panics in `create_rgba_palette` when the
/// length is not (slice index out of range). Upstream rejects such chunks since
/// commit 8330c2c6 of image-png, which is not in a release yet. Drop this when
/// the `png` version in use has that fix: the test
/// `png_still_panics_on_the_fixtures_without_the_check` fails when it does.
fn plte_lengths_ok(bytes: &[u8]) -> bool {
    let mut at = 8usize; // after the signature
    while let Some(head) = bytes.get(at..at.saturating_add(8)) {
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        if &head[4..8] == b"PLTE" && len % 3 != 0 {
            return false;
        }
        if &head[4..8] == b"IEND" {
            break;
        }
        // Length, type, data, CRC; a length that runs past the end ends the scan.
        let Some(next) = usize::try_from(len)
            .ok()
            .and_then(|l| at.checked_add(12)?.checked_add(l))
        else {
            break;
        };
        at = next;
    }
    true
}

fn png(bytes: &[u8]) -> Result<Decoded, Status> {
    if !plte_lengths_ok(bytes) {
        return Err(Status::Corrupt);
    }
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    // Palette and low bit depths expanded, 16-bit stripped to 8.
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|_| Status::Corrupt)?;
    let (w, h) = {
        let info = reader.info();
        (u64::from(info.width), u64::from(info.height))
    };
    let out_len = check_dims(w, h)?;
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(Status::Internal);
    }
    let needed = reader.output_buffer_size().ok_or(Status::TooLarge)?;
    if needed > out_len {
        return Err(Status::TooLarge);
    }
    let mut buf = vec![0u8; needed];
    let frame = reader.next_frame(&mut buf).map_err(|_| Status::Corrupt)?;
    let src = &buf[..frame.buffer_size()];
    let mut rgba = Vec::with_capacity(out_len);
    match color {
        png::ColorType::Rgba => rgba.extend_from_slice(src),
        png::ColorType::Rgb => {
            for p in src.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in src {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in src.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        png::ColorType::Indexed => return Err(Status::Internal),
    }
    if rgba.len() != out_len {
        return Err(Status::Corrupt);
    }
    Ok(Decoded {
        width: dim32(w)?,
        height: dim32(h)?,
        rgba,
    })
}

fn jpeg(bytes: &[u8]) -> Result<Decoded, Status> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    let options = DecoderOptions::default()
        .set_max_width(usize::try_from(MAX_SIDE).map_err(|_| Status::Internal)?)
        .set_max_height(usize::try_from(MAX_SIDE).map_err(|_| Status::Internal)?)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    dec.decode_headers().map_err(|_| Status::Corrupt)?;
    let (w, h) = dec.dimensions().ok_or(Status::Corrupt)?;
    let (w, h) = (w as u64, h as u64);
    let out_len = check_dims(w, h)?;
    let rgba = dec.decode().map_err(|_| Status::Corrupt)?;
    if rgba.len() != out_len {
        return Err(Status::Corrupt);
    }
    Ok(Decoded {
        width: dim32(w)?,
        height: dim32(h)?,
        rgba,
    })
}

fn webp(bytes: &[u8]) -> Result<Decoded, Status> {
    let mut dec = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|_| Status::Corrupt)?;
    let (w, h) = dec.dimensions();
    let (w, h) = (u64::from(w), u64::from(h));
    let out_len = check_dims(w, h)?;
    let alpha = dec.has_alpha();
    let needed = dec.output_buffer_size().ok_or(Status::TooLarge)?;
    if needed > out_len {
        return Err(Status::TooLarge);
    }
    let mut buf = vec![0u8; needed];
    if dec.is_animated() {
        dec.read_frame(&mut buf).map_err(|_| Status::Corrupt)?;
    } else {
        dec.read_image(&mut buf).map_err(|_| Status::Corrupt)?;
    }
    let rgba = if alpha {
        buf
    } else {
        let mut v = Vec::with_capacity(out_len);
        for p in buf.as_chunks::<3>().0 {
            v.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        v
    };
    if rgba.len() != out_len {
        return Err(Status::Corrupt);
    }
    Ok(Decoded {
        width: dim32(w)?,
        height: dim32(h)?,
        rgba,
    })
}

#[allow(dead_code)]
const _: () = assert!(MAX_PIXELS == 32_000_000);
