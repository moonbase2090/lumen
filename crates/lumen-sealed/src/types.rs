//! The sealed frame types, their capability bytes and their payloads.
//!
//! This is the phase 0.5 experiment profile of the frame type table in the
//! spec's transport section, not a normative encoding. The capability byte of
//! a sealed frame is fixed by its type, so [`capability_of`] is the one place
//! that says what a frame of a type must be sealed under.

/// Layout, from a sealed multiplexer layer.
pub const KIND_LAYOUT: u8 = 0x01;
/// One pane container, from a sealed multiplexer layer.
pub const KIND_PANE_CONTAINER: u8 = 0x02;
/// A command starts.
pub const KIND_COMMAND_START: u8 = 0x10;
/// A prompt boundary.
pub const KIND_PROMPT: u8 = 0x11;
/// A command ends, with its exit status.
pub const KIND_COMMAND_END: u8 = 0x12;
/// A capability is granted or revoked.
pub const KIND_GRANT: u8 = 0x20;
/// A clipboard write is requested.
pub const KIND_CLIPBOARD_WRITE: u8 = 0x21;
/// A notification.
pub const KIND_NOTIFICATION: u8 = 0x22;
/// A staged-content marker.
pub const KIND_MARKER: u8 = 0x30;

/// No capability.
pub const CAP_NONE: u8 = 0;
/// The `side-channel` capability.
pub const CAP_SIDE_CHANNEL: u8 = 5;
/// The `clipboard-write` capability.
pub const CAP_CLIPBOARD_WRITE: u8 = 7;
/// The `notifications` capability.
pub const CAP_NOTIFICATIONS: u8 = 8;
/// The highest capability number.
pub const CAP_MAX: u8 = 8;

/// The most bytes of clipboard text in one frame.
pub const MAX_CLIPBOARD: usize = 65536;
/// The most bytes of notification text in one frame.
pub const MAX_NOTIFICATION: usize = 4096;

/// The capability byte a frame of `kind` is sealed under, or `None` for a type
/// that is not in the table. A frame of an unknown type is refused after its
/// header verifies, so no capability is looked up for it.
#[must_use]
pub fn capability_of(kind: u8) -> Option<u8> {
    match kind {
        KIND_LAYOUT | KIND_PANE_CONTAINER | KIND_COMMAND_START | KIND_PROMPT | KIND_COMMAND_END
        | KIND_GRANT => Some(CAP_NONE),
        KIND_CLIPBOARD_WRITE => Some(CAP_CLIPBOARD_WRITE),
        KIND_NOTIFICATION => Some(CAP_NOTIFICATIONS),
        KIND_MARKER => Some(CAP_SIDE_CHANNEL),
        _ => None,
    }
}

/// A parsed sealed payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A layout; its payload is parsed by the multiplexer model.
    Layout(Vec<u8>),
    /// A pane container; its payload is parsed by the multiplexer model.
    PaneContainer(Vec<u8>),
    /// A command starts.
    CommandStart {
        /// The command id.
        id: u32,
    },
    /// A prompt boundary.
    Prompt,
    /// A command ends.
    CommandEnd {
        /// The command id.
        id: u32,
        /// The exit status as the shell reported it.
        status: i32,
    },
    /// A capability is granted or revoked.
    Grant {
        /// True to grant, false to revoke.
        grant: bool,
        /// The capability number, 1 to 8.
        capability: u8,
    },
    /// A clipboard write request.
    ClipboardWrite(String),
    /// A notification.
    Notification(String),
    /// A staged-content marker.
    Marker {
        /// The one-shot id.
        id: u32,
        /// SHA-256 of the staged content.
        hash: [u8; 32],
    },
}

/// Why a payload did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// The type is not in the table.
    UnknownKind,
    /// The length does not match the type.
    Length,
    /// A field is out of range.
    Field,
    /// A string is not valid UTF-8 or is over its limit.
    Text,
}

fn be32(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?))
}

fn text(bytes: &[u8], max: usize) -> Result<String, Malformed> {
    if bytes.len() > max {
        return Err(Malformed::Text);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Malformed::Text)
}

/// Parse the payload of a sealed frame of `kind`.
///
/// # Errors
///
/// Returns why the payload is malformed. The terminal refuses a malformed
/// frame as a failed authenticated frame.
pub fn parse(kind: u8, payload: &[u8]) -> Result<Frame, Malformed> {
    match kind {
        KIND_LAYOUT => Ok(Frame::Layout(payload.to_vec())),
        KIND_PANE_CONTAINER => Ok(Frame::PaneContainer(payload.to_vec())),
        KIND_COMMAND_START => match payload {
            [a, b, c, d] => Ok(Frame::CommandStart {
                id: u32::from_be_bytes([*a, *b, *c, *d]),
            }),
            _ => Err(Malformed::Length),
        },
        KIND_PROMPT if payload.is_empty() => Ok(Frame::Prompt),
        KIND_PROMPT => Err(Malformed::Length),
        KIND_COMMAND_END => {
            if payload.len() != 8 {
                return Err(Malformed::Length);
            }
            let id = be32(payload).ok_or(Malformed::Length)?;
            let status = be32(&payload[4..]).ok_or(Malformed::Length)?;
            Ok(Frame::CommandEnd {
                id,
                status: i32::from_be_bytes(status.to_be_bytes()),
            })
        }
        KIND_GRANT => match payload {
            [action @ (0 | 1), capability @ 1..=CAP_MAX] => Ok(Frame::Grant {
                grant: *action == 1,
                capability: *capability,
            }),
            [_, _] => Err(Malformed::Field),
            _ => Err(Malformed::Length),
        },
        KIND_CLIPBOARD_WRITE => text(payload, MAX_CLIPBOARD).map(Frame::ClipboardWrite),
        KIND_NOTIFICATION => {
            let len = payload.get(..2).ok_or(Malformed::Length)?;
            let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
            let body = payload.get(2..).ok_or(Malformed::Length)?;
            if body.len() != len {
                return Err(Malformed::Length);
            }
            text(body, MAX_NOTIFICATION).map(Frame::Notification)
        }
        KIND_MARKER => {
            if payload.len() != 36 {
                return Err(Malformed::Length);
            }
            let id = be32(payload).ok_or(Malformed::Length)?;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&payload[4..]);
            Ok(Frame::Marker { id, hash })
        }
        _ => Err(Malformed::UnknownKind),
    }
}
