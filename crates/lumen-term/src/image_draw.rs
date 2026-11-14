//! Drawing a decoded image in its container: the size it is drawn at and the
//! blit of its pixels into a frame. This module and `frame.rs` are the only
//! places the terminal reads decoded image bytes for drawing, and both are in
//! the image path's build identity.

use crate::frame::Frame;
use crate::images::Pixels;

/// The most rows of cells a decoded image is drawn at in its container.
pub const IMAGE_MAX_ROWS: u64 = 40;

/// The height in pixels of an image block's container: its border, the label
/// row and the image.
#[must_use]
pub fn block_height(pixels: &Pixels, columns: usize, cell: (u32, u32), scale: u32) -> u32 {
    let (_, h) = size(pixels, columns, cell);
    2 * scale + cell.1 + h
}

/// Draw an image block's picture with its top left at `at`, in a container
/// `columns` cells wide.
pub fn paint_block(
    frame: &mut Frame,
    at: (u32, u32),
    columns: usize,
    cell: (u32, u32),
    pixels: &Pixels,
) {
    let drawn = size(pixels, columns, cell);
    paint_pixels(frame, at, drawn, pixels);
}

/// The size a decoded image is drawn at in a container `columns` wide: its
/// own size, reduced to fit the width inside the container and
/// [`IMAGE_MAX_ROWS`] rows, keeping its proportions, never enlarged. The
/// size the producer declared is not used.
#[must_use]
pub fn size(pixels: &Pixels, columns: usize, cell: (u32, u32)) -> (u32, u32) {
    let max_w =
        u64::from(u32::try_from(columns.saturating_sub(2).max(1)).unwrap_or(1)) * u64::from(cell.0);
    let max_h = IMAGE_MAX_ROWS * u64::from(cell.1);
    let (w, h) = (
        u64::from(pixels.width.max(1)),
        u64::from(pixels.height.max(1)),
    );
    // The largest factor, as a fraction, that fits both limits and is at most 1.
    let (mut num, mut den) = (1u64, 1u64);
    for (limit, side) in [(max_w, w), (max_h, h)] {
        if side * num > limit * den {
            (num, den) = (limit, side);
        }
    }
    let fit = |side: u64| u32::try_from((side * num / den).max(1)).unwrap_or(u32::MAX);
    (fit(w), fit(h))
}

/// Draw decoded premultiplied RGBA scaled (nearest neighbour) to `w` by `h`
/// at `x`, `y`, over what is already in the frame.
fn paint_pixels(frame: &mut Frame, at: (u32, u32), size: (u32, u32), src: &Pixels) {
    let (left, top) = at;
    let (width, height) = size;
    for dy in 0..height {
        let from_row = u64::from(dy) * u64::from(src.height) / u64::from(height);
        for dx in 0..width {
            let from_col = u64::from(dx) * u64::from(src.width) / u64::from(width);
            let index = (from_row * u64::from(src.width) + from_col) * 4;
            let Some(px) = usize::try_from(index)
                .ok()
                .and_then(|i| src.rgba_bytes().get(i..i + 4))
            else {
                continue;
            };
            let inverse = 255 - u32::from(px[3]);
            let under = frame.pixel(left + dx, top + dy).unwrap_or([0, 0, 0]);
            let mut out = [0u8; 3];
            for (channel, slot) in out.iter_mut().enumerate() {
                let value =
                    u32::from(px[channel]) + (u32::from(under[channel]) * inverse + 127) / 255;
                *slot = u8::try_from(value.min(255)).unwrap_or(255);
            }
            frame.put(left + dx, top + dy, out);
        }
    }
}
