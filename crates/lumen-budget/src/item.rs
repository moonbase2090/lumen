//! The checks of one item against the limit that applies to it alone.

use crate::limits;

/// Whether a frame of `len` bytes is within the frame size limit. Images on
/// Track B have a larger one.
#[must_use]
pub fn frame_size_ok(len: u64, image_on_track_b: bool) -> bool {
    let max = if image_on_track_b {
        limits::IMAGE_FRAME_BYTES_TRACK_B
    } else {
        limits::FRAME_BYTES
    };
    len <= max
}

/// Whether a payload nested `depth` deep is within the limit.
#[must_use]
pub fn nesting_ok(depth: u64) -> bool {
    depth <= limits::NESTING_DEPTH
}

/// Whether a table of `columns` and `cells` is within the limits. The cell
/// count is the table's own, not a product the producer could make overflow.
#[must_use]
pub fn table_ok(columns: u64, cells: u64) -> bool {
    columns <= limits::TABLE_COLUMNS && cells <= limits::TABLE_CELLS
}

/// Whether a grid of `columns` by `rows` is within the window size limit, and
/// not empty.
#[must_use]
pub fn grid_size_ok(columns: u64, rows: u64) -> bool {
    (1..=limits::GRID_COLUMNS).contains(&columns) && (1..=limits::GRID_ROWS).contains(&rows)
}

/// Whether `layers` layers are within the limit.
#[must_use]
pub fn layers_ok(layers: u64) -> bool {
    layers <= limits::LAYERS
}

/// Why an image's declared size is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageRefused {
    /// A side is zero or over the side limit.
    Side,
    /// Width times height is over the pixel limit.
    Pixels,
}

/// What an image of a declared size reserves and may be read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageReservation {
    /// Width times height times 4, reserved before any data is read.
    pub bytes: u64,
    /// The most the terminal reads from the decoder for it.
    pub read_limit: u64,
}

/// The reservation for an image of a declared `width` by `height`, or why it is
/// refused. The declared size is a hint until the decoder has bounded it, but a
/// declaration over the limits is refused whatever the decoder says.
///
/// # Errors
///
/// Returns why the size is outside the image limits.
pub fn image_reservation(width: u64, height: u64) -> Result<ImageReservation, ImageRefused> {
    if !(1..=limits::IMAGE_SIDE).contains(&width) || !(1..=limits::IMAGE_SIDE).contains(&height) {
        return Err(ImageRefused::Side);
    }
    // Both sides are at most 65535, so the product fits in 64 bits.
    let pixels = width * height;
    if pixels > limits::IMAGE_PIXELS {
        return Err(ImageRefused::Pixels);
    }
    let bytes = pixels * 4;
    Ok(ImageReservation {
        bytes,
        read_limit: bytes + limits::IMAGE_READ_SLACK,
    })
}
