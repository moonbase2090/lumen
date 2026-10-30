//! Console checkpoints: a block of bytes written as text lines, and read back
//! from console text that has other lines mixed in.
//!
//! A machine with no credentials and no network path out can still write to its
//! serial console, and the launcher can read the last 64 KiB of it. A checkpoint
//! is one self-contained block in that text:
//!
//! ```text
//! PREFIX CKPT-BEGIN id=ID seq=N kind=KIND bytes=B chunks=C sha256=HEX
//! PREFIX CKPT-DATA id=ID n=0 BASE64
//! ...
//! PREFIX CKPT-END id=ID
//! ```
//!
//! The reader looks for the markers anywhere in a line, so a timestamp or a log
//! prefix in front of them does not matter. Of all the blocks in the text it
//! returns the one with the highest `seq` whose chunks are all present and whose
//! SHA-256 matches; a block cut off by the console limit, or damaged, is skipped
//! and an older complete one is used. A block larger than [`MAX_BYTES`] is
//! refused, so that at least one whole block fits in the console window.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Largest payload a block may carry. Base64 makes it a third bigger, and with
/// the line prefixes a block is about 29.5 KB, so that any 64 KiB window of
/// console text, wherever it starts, holds at least one whole block when
/// blocks follow one another (a block under 32 KiB is the condition).
pub const MAX_BYTES: usize = 21_000;
/// Base64 characters per data line.
pub const CHUNK_CHARS: usize = 900;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Standard base64 with padding.
#[must_use]
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes standard base64 with padding; `None` for anything else.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // 6-bit values and masked bytes
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let val = |c: u8| B64.iter().position(|&x| x == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (i, quad) in bytes.chunks(4).enumerate() {
        let last = i + 1 == bytes.len() / 4;
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (j, &c) in quad.iter().enumerate() {
            n = (n << 6) | if j >= 4 - pad { 0 } else { val(c)? };
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Why a block could not be written.
#[derive(Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The payload is bigger than [`MAX_BYTES`].
    TooBig(usize),
    /// `id` or `kind` has a character outside `[A-Za-z0-9._-]`, or `prefix` has whitespace.
    BadWord,
}

fn word_ok(w: &str) -> bool {
    !w.is_empty()
        && w.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The block for `data`, one line per element, each starting with `prefix`.
///
/// # Errors
/// [`EncodeError::TooBig`] when `data` is over [`MAX_BYTES`]; [`EncodeError::BadWord`]
/// for an unusable `prefix`, `id` or `kind`.
pub fn encode(
    prefix: &str,
    id: &str,
    seq: u64,
    kind: &str,
    data: &[u8],
) -> Result<Vec<String>, EncodeError> {
    if !word_ok(prefix) || !word_ok(id) || !word_ok(kind) {
        return Err(EncodeError::BadWord);
    }
    if data.len() > MAX_BYTES {
        return Err(EncodeError::TooBig(data.len()));
    }
    let b64 = base64_encode(data);
    let chunks: Vec<&str> = b64
        .as_bytes()
        .chunks(CHUNK_CHARS)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let mut lines = vec![format!(
        "{prefix} CKPT-BEGIN id={id} seq={seq} kind={kind} bytes={} chunks={} sha256={}",
        data.len(),
        chunks.len(),
        hex(&Sha256::digest(data))
    )];
    for (n, c) in chunks.iter().enumerate() {
        lines.push(format!("{prefix} CKPT-DATA id={id} n={n} {c}"));
    }
    lines.push(format!("{prefix} CKPT-END id={id}"));
    Ok(lines)
}

/// A block read back from console text.
#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    /// The block's id.
    pub id: String,
    /// Its sequence number.
    pub seq: u64,
    /// Its kind word.
    pub kind: String,
    /// The bytes it carried.
    pub data: Vec<u8>,
}

#[derive(Default)]
struct Partial {
    seq: u64,
    kind: String,
    bytes: usize,
    chunks: usize,
    sha: String,
    begun: bool,
    ended: bool,
    data: BTreeMap<usize, String>,
    conflict: bool,
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|w| w.strip_prefix(key))
}

/// Every complete, verified block in `text`, oldest first by `seq`.
#[must_use]
pub fn decode_all(text: &str) -> Vec<Block> {
    let mut parts: BTreeMap<String, Vec<Partial>> = BTreeMap::new();
    for line in text.lines() {
        let Some(at) = line.find("CKPT-") else {
            continue;
        };
        let rest = &line[at..];
        let Some(id) = field(rest, "id=") else {
            continue;
        };
        let list = parts.entry(id.to_string()).or_default();
        if rest.starts_with("CKPT-BEGIN") {
            let mut p = Partial {
                begun: true,
                ..Partial::default()
            };
            p.seq = field(rest, "seq=")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            p.kind = field(rest, "kind=").unwrap_or("").to_string();
            p.bytes = field(rest, "bytes=")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            p.chunks = field(rest, "chunks=")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            p.sha = field(rest, "sha256=").unwrap_or("").to_string();
            list.push(p);
        } else if let Some(p) = list.last_mut() {
            if rest.starts_with("CKPT-DATA") {
                let mut words = rest.split_whitespace();
                let n = words
                    .nth(2)
                    .and_then(|w| w.strip_prefix("n="))
                    .and_then(|v| v.parse::<usize>().ok());
                let body = words.next();
                if let (Some(n), Some(body)) = (n, body) {
                    if p.data.get(&n).is_some_and(|old| old != body) {
                        p.conflict = true;
                    }
                    p.data.insert(n, body.to_string());
                }
            } else if rest.starts_with("CKPT-END") {
                p.ended = true;
            }
        }
    }
    let mut blocks = Vec::new();
    for (id, list) in parts {
        for p in list {
            if !p.begun || !p.ended || p.conflict || p.data.len() != p.chunks || p.bytes > MAX_BYTES
            {
                continue;
            }
            if (0..p.chunks).any(|n| !p.data.contains_key(&n)) {
                continue;
            }
            let joined: String = p.data.values().cloned().collect();
            let Some(data) = base64_decode(&joined) else {
                continue;
            };
            if data.len() != p.bytes || hex(&Sha256::digest(&data)) != p.sha {
                continue;
            }
            blocks.push(Block {
                id: id.clone(),
                seq: p.seq,
                kind: p.kind,
                data,
            });
        }
    }
    blocks.sort_by_key(|b| b.seq);
    blocks
}

/// The newest verified block in `text`, if any.
#[must_use]
pub fn decode_newest(text: &str) -> Option<Block> {
    decode_all(text).pop()
}
