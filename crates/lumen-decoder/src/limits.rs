//! Numeric limits shared by the worker and the host.

/// Memory limit of the worker, in bytes (512 MiB).
pub const MEMORY_BYTES: u64 = 512 * 1024 * 1024;
/// CPU seconds the worker may use for one image.
pub const CPU_SECONDS: u64 = 2;
/// Wall-clock limit the host enforces on a worker, in milliseconds.
pub const WALL_MILLIS: u64 = 4000;
/// Largest encoded input, in bytes (64 MiB).
pub const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
/// Largest image, in pixels (16 megapixels). Four bytes a pixel, that is 64,000,000
/// bytes, which fits the share of one unsigned producer (25% of 256 MiB).
pub const MAX_PIXELS: u64 = 16_000_000;
/// Largest side, in pixels.
pub const MAX_SIDE: u64 = 65535;
/// Slack the host allows on top of the reserved pixel bytes when reading.
pub const READ_SLACK: u64 = 4096;
/// Bytes of the worker's stderr that are kept.
pub const STDERR_KEEP: usize = 4096;

/// Checks a width and height against the side and pixel limits, using 64-bit
/// checked arithmetic, and returns the RGBA byte count.
#[must_use]
pub fn rgba_bytes(width: u64, height: u64) -> Option<u64> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let pixels = width.checked_mul(height)?;
    if pixels > MAX_PIXELS {
        return None;
    }
    pixels.checked_mul(4)
}
