//! Track 0 frame parser.
//!
//! Track 0 frames are unsealed: any program may write them, and they carry no
//! authority. This crate turns a detected frame (type byte and payload) into an
//! inert block, or into an error. A caller shows an error as escaped text with
//! [`escaped_text`] and does nothing else with it.
//!
//! The byte layout is the phase 0.5 experiment profile and is not normative.
//! Unsealed type bytes have the high bit set. Sealed types never do, so neither
//! parser can accept the other's frames (domain separation).

#![forbid(unsafe_code)]

/// Longest string a Track 0 block may carry.
pub const MAX_STRING: usize = 4096;

/// Most bytes of a failed frame shown as text.
pub const MAX_ESCAPED: usize = 256;

/// First type byte of the unsealed range.
pub const UNSEALED_MIN: u8 = 0x80;

/// Type byte of a paragraph block.
pub const KIND_PARAGRAPH: u8 = 0x80;
/// Type byte of a heading block.
pub const KIND_HEADING: u8 = 0x81;
/// Type byte of a code block.
pub const KIND_CODE: u8 = 0x82;

/// An inert rich block. It holds text only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A paragraph of text.
    Paragraph(String),
    /// A heading with a level from 1 to 6.
    Heading {
        /// Heading level, 1 to 6.
        level: u8,
        /// Heading text.
        text: String,
    },
    /// Monospace text with an optional language tag.
    Code {
        /// Language tag, empty when absent.
        lang: String,
        /// The code text.
        text: String,
    },
}

/// Why a frame was not accepted. Every error renders as escaped text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The type byte is in the sealed range. An unsealed parser never accepts it.
    SealedKind,
    /// The type byte is unsealed but not one this parser knows.
    UnknownKind,
    /// The payload ended early or has bytes left over.
    Malformed,
    /// A string is over [`MAX_STRING`].
    StringTooLong,
    /// A string is not valid UTF-8.
    NotUtf8,
    /// A heading level is outside 1 to 6.
    BadLevel,
}

/// Read a one-string field: a 2-byte big-endian length, then that many bytes.
fn take_string<'a>(input: &mut &'a [u8]) -> Result<&'a str, Error> {
    let (len, rest) = input.split_first_chunk::<2>().ok_or(Error::Malformed)?;
    let len = usize::from(u16::from_be_bytes(*len));
    if len > MAX_STRING {
        return Err(Error::StringTooLong);
    }
    if rest.len() < len {
        return Err(Error::Malformed);
    }
    let (text, rest) = rest.split_at(len);
    *input = rest;
    std::str::from_utf8(text).map_err(|_| Error::NotUtf8)
}

fn finish(input: &[u8]) -> Result<(), Error> {
    if input.is_empty() {
        Ok(())
    } else {
        Err(Error::Malformed)
    }
}

/// Parse one Track 0 frame.
///
/// # Errors
///
/// Returns an [`Error`] for a sealed or unknown type, a malformed payload, a
/// string over [`MAX_STRING`], invalid UTF-8 or a bad heading level.
pub fn parse(kind: u8, payload: &[u8]) -> Result<Block, Error> {
    if kind < UNSEALED_MIN {
        return Err(Error::SealedKind);
    }
    let mut input = payload;
    let block = match kind {
        KIND_PARAGRAPH => Block::Paragraph(take_string(&mut input)?.to_owned()),
        KIND_HEADING => {
            let (level, rest) = input.split_first().ok_or(Error::Malformed)?;
            if !(1..=6).contains(level) {
                return Err(Error::BadLevel);
            }
            input = rest;
            Block::Heading {
                level: *level,
                text: take_string(&mut input)?.to_owned(),
            }
        }
        KIND_CODE => {
            let lang = take_string(&mut input)?.to_owned();
            let text = take_string(&mut input)?.to_owned();
            Block::Code { lang, text }
        }
        _ => return Err(Error::UnknownKind),
    };
    finish(input)?;
    Ok(block)
}

/// Render a failed frame as visible text: at most [`MAX_ESCAPED`] bytes, with
/// ESC shown as `␛` and every other control or non-printable byte as `\xNN`,
/// followed by the total byte count. The result contains no control character.
#[must_use]
pub fn escaped_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes.iter().take(MAX_ESCAPED) {
        match b {
            0x1b => out.push('\u{241b}'),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(b)),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!(" [{} bytes]", bytes.len()));
    out
}
