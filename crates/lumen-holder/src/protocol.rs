//! The messages on the seal channel.
//!
//! Every message is a single datagram of at most 128 bytes with a fixed
//! layout: a version byte, a type byte, a 32-bit message number, then
//! fixed-width fields. There are no free-text fields. A message with an
//! unknown version or type, the wrong length, or one that was cut short by the
//! size limit is dropped and counted; the holder never evaluates or expands
//! anything in it.
//!
//! Only `ready` exists so far. Seal and open requests come with the key.

/// The most bytes a message may have; a longer datagram is truncated and refused.
pub const MAX_MESSAGE: usize = 128;
/// The only version.
pub const VERSION: u8 = 1;
/// Type 1: the shell integration is ready to use its holder.
pub const READY: u8 = 1;
/// A `ready` message is exactly this long: version, type, number.
pub const READY_LEN: usize = 6;

/// A message the holder understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// The integration says it is ready. One is accepted per layer.
    Ready {
        /// The message number.
        number: u32,
    },
}

/// Why a message was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// The datagram did not fit in [`MAX_MESSAGE`] bytes.
    Truncated,
    /// Shorter than the two-byte header.
    TooShort,
    /// A version this holder does not speak.
    Version,
    /// A type this holder does not know.
    Type,
    /// The wrong length for its type.
    Length,
}

/// Read a message. `truncated` is set when the kernel cut the datagram short.
///
/// # Errors
///
/// Returns why the message is dropped.
pub fn parse(bytes: &[u8], truncated: bool) -> Result<Message, Malformed> {
    if truncated || bytes.len() > MAX_MESSAGE {
        return Err(Malformed::Truncated);
    }
    let [version, kind, rest @ ..] = bytes else {
        return Err(Malformed::TooShort);
    };
    if *version != VERSION {
        return Err(Malformed::Version);
    }
    match *kind {
        READY => {
            let number: [u8; 4] = rest.try_into().map_err(|_| Malformed::Length)?;
            Ok(Message::Ready {
                number: u32::from_be_bytes(number),
            })
        }
        _ => Err(Malformed::Type),
    }
}

/// The bytes of a `ready` message, for the integration's side and for tests.
#[must_use]
pub fn ready(number: u32) -> [u8; READY_LEN] {
    let n = number.to_be_bytes();
    [VERSION, READY, n[0], n[1], n[2], n[3]]
}
