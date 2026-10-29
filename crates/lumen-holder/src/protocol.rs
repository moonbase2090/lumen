//! The messages on the seal channel.
//!
//! Every message is a single datagram of at most 128 bytes with a fixed
//! layout: a version byte, a type byte, a 32-bit message number, then
//! fixed-width fields. There are no free-text fields. A message with an
//! unknown version or type, the wrong length, or one that was cut short by the
//! size limit is dropped and counted; the holder never evaluates or expands
//! anything in it.
//!
//! On the seal channel only `ready` exists so far; seal and open requests come
//! later. The spawn socketpair carries one message, the key, from the terminal.

use zeroize::Zeroizing;

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

/// Type 2: the layer's key, sent by the terminal on the spawn socketpair.
pub const KEY: u8 = 2;
/// A `key` message is exactly this long: version, type, number, layer, 32 bytes.
pub const KEY_LEN: usize = 42;

/// The key message. The root is secret: it is zeroized when dropped and never
/// printed.
pub struct KeyMessage {
    /// The message number.
    pub number: u32,
    /// The layer the key is for.
    pub layer: u32,
    /// The layer's root key.
    pub root: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for KeyMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyMessage")
            .field("number", &self.number)
            .field("layer", &self.layer)
            .field("root", &"<redacted>")
            .finish()
    }
}

/// Read a key message from the spawn socketpair.
///
/// # Errors
///
/// Returns why the message is dropped.
pub fn parse_key(bytes: &[u8], truncated: bool) -> Result<KeyMessage, Malformed> {
    if truncated || bytes.len() > MAX_MESSAGE {
        return Err(Malformed::Truncated);
    }
    let [version, kind, rest @ ..] = bytes else {
        return Err(Malformed::TooShort);
    };
    if *version != VERSION {
        return Err(Malformed::Version);
    }
    if *kind != KEY {
        return Err(Malformed::Type);
    }
    let fields: &[u8; KEY_LEN - 2] = rest.try_into().map_err(|_| Malformed::Length)?;
    let (number, rest) = fields.split_at(4);
    let (layer, root) = rest.split_at(4);
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(root);
    Ok(KeyMessage {
        number: u32::from_be_bytes(number.try_into().map_err(|_| Malformed::Length)?),
        layer: u32::from_be_bytes(layer.try_into().map_err(|_| Malformed::Length)?),
        root: key,
    })
}

/// The bytes of a `key` message, for the terminal's side and for tests. The
/// buffer is zeroized when dropped.
#[must_use]
pub fn key_message(number: u32, layer: u32, root: &[u8; 32]) -> Zeroizing<[u8; KEY_LEN]> {
    let mut m = Zeroizing::new([0u8; KEY_LEN]);
    m[0] = VERSION;
    m[1] = KEY;
    m[2..6].copy_from_slice(&number.to_be_bytes());
    m[6..10].copy_from_slice(&layer.to_be_bytes());
    m[10..].copy_from_slice(root);
    m
}
