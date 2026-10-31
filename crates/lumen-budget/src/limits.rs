//! The numbers of the resource limits table, the transport limits and the image
//! rules. Sizes are in bytes unless the name says otherwise.

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// Blocks per document.
pub const BLOCKS_PER_DOCUMENT: u64 = 100_000;
/// Patches per block, per second.
pub const PATCHES_PER_BLOCK_PER_SEC: u64 = 10_000;
/// Patches per document, per second.
pub const PATCHES_PER_DOCUMENT_PER_SEC: u64 = 50_000;
/// The shaping cache.
pub const SHAPING_CACHE_BYTES: u64 = 64 * MIB;
/// Columns of a table.
pub const TABLE_COLUMNS: u64 = 256;
/// Cells of a table.
pub const TABLE_CELLS: u64 = 100_000;
/// Grids per document.
pub const GRIDS_PER_DOCUMENT: u64 = 64;
/// Grid cells per document.
pub const GRID_CELLS_PER_DOCUMENT: u64 = 4_000_000;
/// Scrollback per document.
pub const SCROLLBACK_BYTES: u64 = 64 * MIB;
/// Columns of one grid.
pub const GRID_COLUMNS: u64 = 1000;
/// Rows of one grid.
pub const GRID_ROWS: u64 = 500;
/// Frames per producer, per second.
pub const FRAMES_PER_PRODUCER_PER_SEC: u64 = 2_000;
/// Bytes per producer, per second.
pub const BYTES_PER_PRODUCER_PER_SEC: u64 = 8 * MIB;
/// The size of a frame.
pub const FRAME_BYTES: u64 = MIB;
/// The size of an image frame on Track B.
pub const IMAGE_FRAME_BYTES_TRACK_B: u64 = 16 * MIB;
/// Nesting depth of a payload.
pub const NESTING_DEPTH: u64 = 32;
/// Decoded images that are live at once, per session.
pub const DECODED_IMAGE_BYTES: u64 = 256 * MIB;
/// GPU textures, in total.
pub const GPU_TEXTURE_BYTES: u64 = 512 * MIB;
/// Layers.
pub const LAYERS: u64 = 8;
/// Pixels of an image (16 megapixels, as the decoder counts them). Four bytes a
/// pixel, that is 64,000,000 bytes, which fits the share of one unsigned producer.
pub const IMAGE_PIXELS: u64 = 16_000_000;
/// Pixels along either side of an image.
pub const IMAGE_SIDE: u64 = 65_535;
/// Bytes the terminal reads from the decoder beyond width times height times 4.
pub const IMAGE_READ_SLACK: u64 = 4 * KIB;
/// The length of a sliding window, in milliseconds. Every per-second limit uses it.
pub const WINDOW_MS: u64 = 1000;
