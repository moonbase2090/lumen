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
#[must_use]
pub fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}')
}

/// Whether `c` has the Unicode property `Default_Ignorable_Code_Point` (Unicode
/// 16): a code point that draws nothing of its own, such as the zero-width
/// characters, the joiners, the variation selectors, the soft hyphen, the
/// Hangul fillers and the tag characters.
#[must_use]
pub fn is_default_ignorable(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00ad
            | 0x034f
            | 0x061c
            | 0x115f..=0x1160
            | 0x17b4..=0x17b5
            | 0x180b..=0x180f
            | 0x200b..=0x200f
            | 0x202a..=0x202e
            | 0x2060..=0x206f
            | 0x3164
            | 0xfe00..=0xfe0f
            | 0xfeff
            | 0xffa0
            | 0xfff0..=0xfff8
            | 0x1bca0..=0x1bca3
            | 0x1d173..=0x1d17a
            | 0xe0000..=0xe0fff
    )
}

/// Show every default-ignorable and bidi code point in `text` as a visible
/// escape such as `⟨U+200B⟩`, for text the terminal composes into its own trust
/// surfaces (labels, link targets, notification origin text) where nothing may
/// be invisible. Every other character is kept as it is.
#[must_use]
pub fn escape_invisible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_default_ignorable(c) || is_bidi(c) {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("\u{27e8}U+{:04X}\u{27e9}", u32::from(c)),
            );
        } else {
            out.push(c);
        }
    }
    out
}

/// Text outside code, the earlier rule: bidi controls are shown as `\u{..}`
/// escapes, so they cannot reorder what the reader sees.
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

/// Paragraph and heading text: with the `zero-width-text` feature every code
/// point is kept and open bidi controls are closed ([`close_bidi`]); without it
/// the earlier rule applies ([`visible`]).
fn text_outside_code(text: &str) -> String {
    if cfg!(feature = "zero-width-text") {
        close_bidi(text)
    } else {
        visible(text)
    }
}

/// The language label of a code block: with the feature, default-ignorable and
/// bidi code points are shown as escapes ([`escape_invisible`]); without it the
/// earlier rule applies.
fn label_text(text: &str) -> String {
    if cfg!(feature = "zero-width-text") {
        escape_invisible(text)
    } else {
        visible(text)
    }
}

/// Text outside code keeps every code point, the zero-width and bidi controls
/// included, so that joiners shape and copy returns what was sent. What is
/// closed is bidirectional state: every embedding, override and isolate still
/// open at the end of a line (a paragraph boundary in UAX #9) or of the block
/// is closed there with the matching PDF or PDI, so a control cannot reach
/// into the next line, the next block or the terminal's own text.
fn close_bidi(text: &str) -> String {
    /// What an open control needs to close it.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Open {
        Embedding,
        Isolate,
    }
    fn close_all(out: &mut String, stack: &mut Vec<Open>) {
        while let Some(open) = stack.pop() {
            out.push(match open {
                Open::Embedding => '\u{202c}',
                Open::Isolate => '\u{2069}',
            });
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut stack: Vec<Open> = Vec::new();
    for c in text.chars() {
        match c {
            '\n' => {
                close_all(&mut out, &mut stack);
                out.push(c);
            }
            '\u{202a}'..='\u{202e}' if c != '\u{202c}' => {
                stack.push(Open::Embedding);
                out.push(c);
            }
            '\u{2066}'..='\u{2068}' => {
                stack.push(Open::Isolate);
                out.push(c);
            }
            '\u{202c}' => {
                if stack.last() == Some(&Open::Embedding) {
                    stack.pop();
                }
                out.push(c);
            }
            '\u{2069}' => {
                if stack.contains(&Open::Isolate) {
                    while let Some(open) = stack.pop() {
                        if open == Open::Isolate {
                            break;
                        }
                    }
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    close_all(&mut out, &mut stack);
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
        KIND_PARAGRAPH => Block::Paragraph(text_outside_code(take_string(&mut input)?)),
        KIND_HEADING => {
            let (level, rest) = input.split_first().ok_or(Error::Malformed)?;
            if !(1..=6).contains(level) {
                return Err(Error::BadLevel);
            }
            input = rest;
            Block::Heading {
                level: *level,
                text: text_outside_code(take_string(&mut input)?),
            }
        }
        KIND_CODE => {
            let lang = label_text(take_string(&mut input)?);
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
