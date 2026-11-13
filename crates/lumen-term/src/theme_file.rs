//! Theme files: a palette written as text.
//!
//! A theme file is the small part of TOML a palette needs: `key = "#rrggbb"`
//! on a line, `#` comments and blank lines. `base` names the built-in palette
//! it starts from (`light`, `dark` or `high-contrast`; `light` when it says
//! nothing), and every other key is a colour role of [`Palette`] or one of the
//! ANSI colours, `ansi.0` to `ansi.15`. A role the file does not give keeps the
//! base's colour.
//!
//! ```toml
//! # A dark theme with a green accent.
//! base = "dark"
//! accent = "#7ee2a8"
//! ansi.2 = "#7ee2a8"
//! ```
//!
//! The parse is strict: an unknown key, a key given twice, a value that is not
//! a quoted `#rrggbb` colour or a line that is not `key = value` refuses the
//! whole file, since a half-read theme is worse than none. A theme whose text
//! falls under the spec's 3:1 visibility floor on any surface is refused too
//! ([`Floors::USER`]): runs drawn in it would count as hidden.

use crate::design::{Builtin, Floors, Palette};
use crate::frame::Rgb;
use std::path::Path;

/// The most bytes a theme file may hold.
pub const MAX_BYTES: u64 = 16 * 1024;

/// Why a theme file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// It could not be read, or is not a regular file.
    Unreadable,
    /// It is larger than [`MAX_BYTES`].
    TooLarge,
    /// A line is wrong.
    Line {
        /// The line, counting from 1.
        line: usize,
        /// What was wrong with it.
        why: String,
    },
    /// Its text is under the spec's visibility floor; one entry per miss.
    TooFaint(Vec<String>),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable => write!(f, "the theme file could not be read"),
            Self::TooLarge => write!(f, "the theme file is over {MAX_BYTES} bytes"),
            Self::Line { line, why } => write!(f, "line {line}: {why}"),
            Self::TooFaint(misses) => {
                write!(f, "text would be too faint to show: {}", misses.join("; "))
            }
        }
    }
}

fn colour(value: &str) -> Option<Rgb> {
    let hex = value.strip_prefix("\"#")?.strip_suffix('"')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn slot<'a>(p: &'a mut Palette, key: &str) -> Option<&'a mut Rgb> {
    if let Some(n) = key.strip_prefix("ansi.") {
        // Only the plain decimal forms: `ansi.07` and `ansi.+7` are not keys.
        if n.is_empty()
            || (n.len() > 1 && n.starts_with('0'))
            || !n.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        return p.ansi.get_mut(n.parse::<usize>().ok()?);
    }
    Some(match key {
        "canvas" => &mut p.canvas,
        "surface" => &mut p.surface,
        "sunken" => &mut p.sunken,
        "field" => &mut p.field,
        "chrome" => &mut p.chrome,
        "text" => &mut p.text,
        "text_strong" => &mut p.text_strong,
        "text_muted" => &mut p.text_muted,
        "chrome_text" => &mut p.chrome_text,
        "accent" => &mut p.accent,
        "error" => &mut p.error,
        "caution" => &mut p.caution,
        "control_border" => &mut p.control_border,
        "notice" => &mut p.notice,
        "notice_border" => &mut p.notice_border,
        _ => return None,
    })
}

/// The palette a theme file's text gives.
///
/// # Errors
///
/// [`Refused::Line`] for the first line that is wrong, and
/// [`Refused::TooFaint`] for a theme whose text is under the spec's floor.
pub fn parse(text: &str) -> Result<Palette, Refused> {
    let refuse = |line: usize, why: &str| Refused::Line {
        line: line + 1,
        why: why.to_owned(),
    };
    let mut entries: Vec<(usize, &str, &str)> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(refuse(n, "not `key = value`"));
        };
        let (key, value) = (key.trim(), value.trim());
        if entries.iter().any(|(_, k, _)| *k == key) {
            return Err(refuse(n, &format!("`{key}` is given twice")));
        }
        entries.push((n, key, value));
    }
    let mut palette = match entries.iter().find(|(_, k, _)| *k == "base") {
        None => Palette::light(),
        Some((n, _, value)) => value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .and_then(Builtin::named)
            .ok_or_else(|| refuse(*n, "`base` is not \"light\", \"dark\" or \"high-contrast\""))?
            .palette(),
    };
    for (n, key, value) in entries {
        if key == "base" {
            continue;
        }
        let Some(target) = slot(&mut palette, key) else {
            return Err(refuse(n, &format!("`{key}` is not a colour role")));
        };
        *target = colour(value)
            .ok_or_else(|| refuse(n, &format!("`{key}` is not a quoted \"#rrggbb\" colour")))?;
    }
    let misses = palette.check(&Floors::USER);
    if misses.is_empty() {
        Ok(palette)
    } else {
        Err(Refused::TooFaint(misses))
    }
}

/// The palette of the theme file at `path`.
///
/// # Errors
///
/// As [`parse`], and [`Refused::Unreadable`] or [`Refused::TooLarge`] for a
/// file that cannot be read or is too big to be a theme.
pub fn load(path: &Path) -> Result<Palette, Refused> {
    let meta = std::fs::metadata(path).map_err(|_| Refused::Unreadable)?;
    if !meta.is_file() {
        return Err(Refused::Unreadable);
    }
    if meta.len() > MAX_BYTES {
        return Err(Refused::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(|_| Refused::Unreadable)?;
    // The size is checked again on the bytes read: the file may have grown.
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Refused::TooLarge);
    }
    let text = String::from_utf8(bytes).map_err(|_| Refused::Line {
        line: 1,
        why: "not UTF-8 text".to_owned(),
    })?;
    parse(&text)
}
