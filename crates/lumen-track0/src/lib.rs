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
        /// The code text, with bidi controls removed.
        text: String,
        /// How many bidi controls were removed from `text`. The terminal shows
        /// this count.
        stripped: usize,
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
    /// A string holds a control character other than newline or tab.
    Control,
}

/// Bidi override, isolate and mark controls (the spec's bidi list).
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}')
}

/// Text outside code: bidi controls are kept visible as `\u{..}` escapes, so
/// they cannot reorder what the reader sees.
fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_bidi(c) {
            let _ =
                std::fmt::Write::write_fmt(&mut out, format_args!("\\u{{{:04x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// Code text: bidi controls are removed and counted.
fn strip_bidi(text: &str) -> (String, usize) {
    let kept: String = text.chars().filter(|&c| !is_bidi(c)).collect();
    let removed = text.chars().count() - kept.chars().count();
    (kept, removed)
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
    let text = std::str::from_utf8(text).map_err(|_| Error::NotUtf8)?;
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Control);
    }
    Ok(text)
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
        KIND_PARAGRAPH => Block::Paragraph(visible(take_string(&mut input)?)),
        KIND_HEADING => {
            let (level, rest) = input.split_first().ok_or(Error::Malformed)?;
            if !(1..=6).contains(level) {
                return Err(Error::BadLevel);
            }
            input = rest;
            Block::Heading {
                level: *level,
                text: visible(take_string(&mut input)?),
            }
        }
        KIND_CODE => {
            let lang = visible(take_string(&mut input)?);
            let (text, stripped) = strip_bidi(take_string(&mut input)?);
            Block::Code {
                lang,
                text,
                stripped,
            }
        }
        _ => return Err(Error::UnknownKind),
    };
    finish(input)?;
    Ok(block)
}

/// Render a failed frame as visible text. The text is at most [`MAX_ESCAPED`]
/// bytes of output: ESC is shown as `␛`, every other control or non-printable
/// byte as `\xNN`, and rendering stops before an escape that would not fit. The
/// total input byte count follows the text. The result contains no control
/// character.
#[must_use]
pub fn escaped_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        let mut piece = String::new();
        match b {
            0x1b => piece.push('\u{241b}'),
            b'\\' => piece.push_str("\\\\"),
            0x20..=0x7e => piece.push(char::from(b)),
            _ => {
                let _ = std::fmt::Write::write_fmt(&mut piece, format_args!("\\x{b:02x}"));
            }
        }
        if out.len() + piece.len() > MAX_ESCAPED {
            break;
        }
        out.push_str(&piece);
    }
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!(" [{} bytes]", bytes.len()));
    out
}
