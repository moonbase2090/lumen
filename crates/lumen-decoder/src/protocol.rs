//! Wire layout of the request and the reply. All integers are little-endian.

/// Magic of a request.
pub const REQUEST_MAGIC: [u8; 4] = *b"LDEC";
/// Magic of a reply.
pub const REPLY_MAGIC: [u8; 4] = *b"LDER";
/// Protocol version.
pub const VERSION: u8 = 1;
/// Size of the request header in bytes.
pub const REQUEST_LEN: usize = 24;
/// Size of the reply header in bytes.
pub const REPLY_LEN: usize = 28;

/// An accepted image format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// PNG.
    Png = 1,
    /// JPEG.
    Jpeg = 2,
    /// WebP.
    Webp = 3,
}

impl Format {
    /// Maps a wire byte to a format.
    #[must_use]
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Png),
            2 => Some(Self::Jpeg),
            3 => Some(Self::Webp),
            _ => None,
        }
    }

    /// Recognises a format from the leading bytes of an image.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }
}

/// The result a worker reports. Statuses other than `Ok` are not crashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Decoded.
    Ok = 0,
    /// The request was malformed.
    Malformed = 1,
    /// Unsupported or mismatched format.
    Unsupported = 2,
    /// Over a size limit.
    TooLarge = 3,
    /// The image is corrupt.
    Corrupt = 4,
    /// Internal failure.
    Internal = 5,
}

impl Status {
    /// Maps a wire byte to a status.
    #[must_use]
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Ok),
            1 => Some(Self::Malformed),
            2 => Some(Self::Unsupported),
            3 => Some(Self::TooLarge),
            4 => Some(Self::Corrupt),
            5 => Some(Self::Internal),
            _ => None,
        }
    }
}

/// A parsed request header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// The format the terminal sniffed.
    pub format: Format,
    /// Declared width, a hint only.
    pub hint_width: u32,
    /// Declared height, a hint only.
    pub hint_height: u32,
    /// Length of the payload that follows.
    pub payload_len: u64,
}

impl Request {
    /// Encodes the header.
    #[must_use]
    pub fn encode(&self) -> [u8; REQUEST_LEN] {
        let mut out = [0u8; REQUEST_LEN];
        out[0..4].copy_from_slice(&REQUEST_MAGIC);
        out[4] = VERSION;
        out[5] = self.format as u8;
        out[8..12].copy_from_slice(&self.hint_width.to_le_bytes());
        out[12..16].copy_from_slice(&self.hint_height.to_le_bytes());
        out[16..24].copy_from_slice(&self.payload_len.to_le_bytes());
        out
    }

    /// Parses a header. Any deviation is `Malformed`, an unknown format is
    /// `Unsupported`.
    ///
    /// # Errors
    /// Returns the status the worker should answer with.
    pub fn parse(b: &[u8; REQUEST_LEN]) -> Result<Self, Status> {
        if b[0..4] != REQUEST_MAGIC || b[4] != VERSION || b[6] != 0 || b[7] != 0 {
            return Err(Status::Malformed);
        }
        let format = Format::from_byte(b[5]).ok_or(Status::Unsupported)?;
        Ok(Self {
            format,
            hint_width: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            hint_height: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
            payload_len: u64::from_le_bytes([
                b[16], b[17], b[18], b[19], b[20], b[21], b[22], b[23],
            ]),
        })
    }
}

/// A reply header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The status byte as sent. It may be one this crate does not know.
    pub status: u8,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row.
    pub stride: u32,
    /// Length of the data that follows.
    pub data_len: u64,
}

impl Reply {
    /// A reply that reports a status and no image.
    #[must_use]
    pub fn status_only(status: Status) -> Self {
        Self {
            status: status as u8,
            width: 0,
            height: 0,
            stride: 0,
            data_len: 0,
        }
    }

    /// Encodes the header.
    #[must_use]
    pub fn encode(&self) -> [u8; REPLY_LEN] {
        let mut out = [0u8; REPLY_LEN];
        out[0..4].copy_from_slice(&REPLY_MAGIC);
        out[4] = self.status;
        out[8..12].copy_from_slice(&self.width.to_le_bytes());
        out[12..16].copy_from_slice(&self.height.to_le_bytes());
        out[16..20].copy_from_slice(&self.stride.to_le_bytes());
        out[20..28].copy_from_slice(&self.data_len.to_le_bytes());
        out
    }

    /// Parses a header, or `None` if the magic or the reserved bytes are wrong.
    #[must_use]
    pub fn parse(b: &[u8; REPLY_LEN]) -> Option<Self> {
        if b[0..4] != REPLY_MAGIC || b[5..8] != [0, 0, 0] {
            return None;
        }
        Some(Self {
            status: b[4],
            width: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            height: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
            stride: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
            data_len: u64::from_le_bytes([b[20], b[21], b[22], b[23], b[24], b[25], b[26], b[27]]),
        })
    }
}
