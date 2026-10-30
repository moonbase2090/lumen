//! The messages on the seal channel.
//!
//! Every message is a single datagram of at most 128 bytes with a fixed
//! layout: a version byte, a type byte, a 32-bit message number, then
//! fixed-width fields. There are no free-text fields. A message with an
//! unknown version or type, the wrong length, or one that was cut short by the
//! size limit is dropped and counted; the holder never evaluates or expands
//! anything in it.
//!
//! On the seal channel the shell sends `ready` and, with the `seal-requests`
//! feature, seal requests; the holder
//! answers a seal request with the sealed frame or a refusal. The spawn
//! socketpair carries one message, the key, from the terminal.
//!
//! A seal request names a frame type and carries that frame's plaintext
//! payload, whose layout the type fixes. Free text is never carried: the
//! largest request is 128 bytes and the largest payload a request can hold is
//! 121 bytes, so the types with text (notification, clipboard write) are not
//! on this channel.

use zeroize::Zeroizing;

/// The most bytes a message may have; a longer datagram is truncated and refused.
pub const MAX_MESSAGE: usize = 128;
/// The only version.
pub const VERSION: u8 = 1;
/// Type 1: the shell integration is ready to use its holder.
pub const READY: u8 = 1;
/// A `ready` message is exactly this long: version, type, number.
pub const READY_LEN: usize = 6;

/// Type 3: the shell asks the holder to seal one frame.
pub const SEAL: u8 = 3;
/// The fixed part of a seal request: version, type, number and frame type.
pub const SEAL_HEADER_LEN: usize = 7;
/// The most payload bytes a seal request can carry.
pub const MAX_SEAL_BODY: usize = MAX_MESSAGE - SEAL_HEADER_LEN;
/// Type 4: the holder's answer to a seal request, carrying the sealed frame.
pub const SEALED: u8 = 4;
/// Type 5: the holder's refusal of a seal request, with a reason byte.
pub const SEAL_REFUSED: u8 = 5;
/// The fixed part of a `sealed` answer: version, type and the request's number.
pub const SEALED_HEADER_LEN: usize = 6;
/// A `seal-refused` answer is exactly this long: version, type, number, reason.
pub const SEAL_REFUSED_LEN: usize = 7;

/// A message the holder understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The integration says it is ready. One is accepted per layer.
    Ready {
        /// The message number.
        number: u32,
    },
    /// The shell asks for one frame to be sealed.
    Seal {
        /// The message number, echoed in the answer.
        number: u32,
        /// The frame type byte.
        kind: u8,
        /// The frame's plaintext payload, as the type fixes it.
        body: Vec<u8>,
    },
}

/// Why the holder would not seal a frame. The byte is what the answer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealRefusal {
    /// The shell has not said it is ready.
    NotReady = 1,
    /// The terminal's key has not arrived.
    NoKey = 2,
    /// The holder does not seal frames of this type for the shell.
    KindNotAllowed = 3,
    /// The payload is not the layout the type fixes.
    BadBody = 4,
    /// The frame could not be sealed (the layer's sequence numbers are used up).
    Failed = 5,
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
        SEAL if cfg!(feature = "seal-requests") => {
            let [a, b, c, d, frame_kind, body @ ..] = rest else {
                return Err(Malformed::Length);
            };
            Ok(Message::Seal {
                number: u32::from_be_bytes([*a, *b, *c, *d]),
                kind: *frame_kind,
                body: body.to_vec(),
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

/// The bytes of a seal request, for the integration's side and for tests.
/// Returns `None` if the payload does not fit in a request.
#[must_use]
pub fn seal_request(number: u32, kind: u8, body: &[u8]) -> Option<Vec<u8>> {
    if body.len() > MAX_SEAL_BODY {
        return None;
    }
    let mut m = vec![VERSION, SEAL];
    m.extend_from_slice(&number.to_be_bytes());
    m.push(kind);
    m.extend_from_slice(body);
    Some(m)
}

/// The bytes of a `sealed` answer: the request's number, then the sealed frame
/// exactly as it goes on the terminal's input. `None` if it would not fit in a
/// message.
#[must_use]
pub fn sealed_answer(number: u32, frame: &[u8]) -> Option<Vec<u8>> {
    if SEALED_HEADER_LEN + frame.len() > MAX_MESSAGE {
        return None;
    }
    let mut m = vec![VERSION, SEALED];
    m.extend_from_slice(&number.to_be_bytes());
    m.extend_from_slice(frame);
    Some(m)
}

/// The bytes of a `seal-refused` answer.
#[must_use]
pub fn refused_answer(number: u32, why: SealRefusal) -> [u8; SEAL_REFUSED_LEN] {
    let n = number.to_be_bytes();
    [VERSION, SEAL_REFUSED, n[0], n[1], n[2], n[3], why as u8]
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
