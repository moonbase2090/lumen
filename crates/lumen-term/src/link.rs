//! Link targets: what the terminal will let a link do, and what it shows first.
//!
//! A link is a visible text and a target. Nothing here opens anything. [`check`]
//! decides what a target may be: only `https`, `http` and `mailto` are acted on,
//! a `file` target is inert text with a path to copy, every other scheme is
//! refused, and a target that does not decode cleanly is refused. What a check
//! returns is exactly what the confirmation shows and what a copy or a paste
//! would carry, so the user confirms what is used.
//!
//! The host of a web target is shown in its ASCII (punycode) form and is never
//! cut. A target with a user name in it is refused, because the part before the
//! `@` is what a lookalike hides behind.

use lumen_track0::{is_bidi, is_default_ignorable};
use std::fmt::Write as _;

/// The longest target, in bytes. A longer one is refused rather than cut, since
/// the confirmation must show all of it.
pub const MAX_TARGET: usize = 2048;

/// Why a target was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Empty, or longer than [`MAX_TARGET`].
    Size,
    /// A control character or a space.
    Control,
    /// A default-ignorable or bidirectional code point, which could hide part of
    /// the target.
    Invisible,
    /// A scheme other than `https`, `http`, `mailto` and `file`, or none.
    Scheme,
    /// A percent escape that is malformed or does not decode to clean text.
    Encoding,
    /// A web target with no host, or a host that is not a valid name.
    Host,
    /// A port that is empty, not a number, or over 65535.
    Port,
    /// A web target with a user name or password in it.
    UserInfo,
    /// A `mailto` address that is empty, has no `@`, or contains `?`, `&` or a
    /// control character.
    Address,
    /// A `mailto` value that contains a control character, including CR and LF.
    MailValue,
}

/// What kind of link an accepted target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An `http` or `https` target.
    Web,
    /// A `mailto` target.
    Mail,
}

/// A target that may be acted on after the user confirms it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Web or mail.
    pub kind: Kind,
    /// The target as it will be used and as the confirmation shows it. For a web
    /// target the host is in punycode; for `mailto` it is re-encoded and holds
    /// only the address, `cc` and `subject`.
    pub shown: String,
    /// The host in ASCII form, for a web target.
    pub host: Option<String>,
    /// The host was not plain ASCII, or has a punycode label: say so.
    pub idn: bool,
    /// The visible text looks like a URL that differs from this target.
    pub mismatch: bool,
}

/// What a target is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Acted on only after a confirmation the terminal draws.
    Confirm(Checked),
    /// A `file` target: inert text, with the decoded path offered to copy.
    Inert {
        /// The decoded path, valid text with no control character.
        path: String,
    },
    /// Not acted on, and why.
    Refused(Refusal),
}

/// Decide what the target `target` of a link with visible text `visible` is.
#[must_use]
pub fn check(target: &str, visible: &str) -> Verdict {
    match parse(target) {
        Ok(Parsed::Link(mut checked)) => {
            checked.mismatch = mismatch(&checked, visible);
            Verdict::Confirm(checked)
        }
        Ok(Parsed::File(path)) => Verdict::Inert { path },
        Err(why) => Verdict::Refused(why),
    }
}

enum Parsed {
    Link(Checked),
    File(String),
}

fn parse(target: &str) -> Result<Parsed, Refusal> {
    if target.is_empty() || target.len() > MAX_TARGET {
        return Err(Refusal::Size);
    }
    for c in target.chars() {
        if c.is_control() || c.is_whitespace() {
            return Err(Refusal::Control);
        }
        if is_default_ignorable(c) || is_bidi(c) {
            return Err(Refusal::Invisible);
        }
    }
    let (scheme, rest) = target.split_once(':').ok_or(Refusal::Scheme)?;
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return Err(Refusal::Scheme);
    }
    match scheme.to_ascii_lowercase().as_str() {
        "https" | "http" => web(&scheme.to_ascii_lowercase(), rest).map(Parsed::Link),
        "mailto" => mail(rest).map(Parsed::Link),
        "file" => file(rest).map(Parsed::File),
        _ => Err(Refusal::Scheme),
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode `text` to clean text: every escape well formed, the bytes
/// valid UTF-8, and no control, default-ignorable or bidirectional code point
/// in the result.
fn decode(text: &str) -> Result<String, Refusal> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hi = bytes.get(i + 1).and_then(|h| hex(*h));
            let lo = bytes.get(i + 2).and_then(|l| hex(*l));
            match (hi, lo) {
                (Some(h), Some(l)) => out.push(h << 4 | l),
                _ => return Err(Refusal::Encoding),
            }
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| Refusal::Encoding)?;
    if decoded.chars().any(char::is_control) {
        return Err(Refusal::Encoding);
    }
    if decoded
        .chars()
        .any(|c| is_default_ignorable(c) || is_bidi(c))
    {
        return Err(Refusal::Invisible);
    }
    Ok(decoded)
}

fn file(rest: &str) -> Result<String, Refusal> {
    let path = rest.strip_prefix("//").unwrap_or(rest);
    // A host in a file target is not interpreted: the path starts at the first `/`.
    let path = match path.find('/') {
        Some(i) => &path[i..],
        None => "",
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    decode(path)
}

fn web(scheme: &str, rest: &str) -> Result<Checked, Refusal> {
    let after = rest.strip_prefix("//").ok_or(Refusal::Host)?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, tail) = after.split_at(end);
    if authority.contains('@') {
        return Err(Refusal::UserInfo);
    }
    let (host_text, port) = split_port(authority)?;
    let (host, idn) = ascii_host(host_text)?;
    // The rest must decode cleanly; it is shown as it was written.
    decode(tail)?;
    let mut shown = format!("{scheme}://{host}");
    if let Some(p) = port {
        shown.push(':');
        shown.push_str(p);
    }
    shown.push_str(tail);
    Ok(Checked {
        kind: Kind::Web,
        shown,
        host: Some(host),
        idn,
        mismatch: false,
    })
}

fn split_port(authority: &str) -> Result<(&str, Option<&str>), Refusal> {
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']').ok_or(Refusal::Host)?;
        let (host, after) = authority.split_at(close + 1);
        match after.strip_prefix(':') {
            Some(p) => (host, Some(p)),
            None if after.is_empty() => (host, None),
            None => return Err(Refusal::Host),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if let Some(p) = port {
        let number = p.parse::<u32>().ok().filter(|n| *n <= 65535);
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || number.is_none() {
            return Err(Refusal::Port);
        }
    }
    Ok((host, port))
}

/// The host in ASCII form, lower case, with each non-ASCII label as punycode,
/// and whether it was an internationalised name.
fn ascii_host(host: &str) -> Result<(String, bool), Refusal> {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ok = !inner.is_empty()
            && inner.contains(':')
            && inner
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
        return if ok {
            Ok((format!("[{}]", inner.to_ascii_lowercase()), false))
        } else {
            Err(Refusal::Host)
        };
    }
    if host.is_empty() || host.len() > 253 * 4 {
        return Err(Refusal::Host);
    }
    let mut labels = Vec::new();
    let mut idn = false;
    for label in host.split('.') {
        if label.is_empty() {
            return Err(Refusal::Host);
        }
        let lower = label.to_lowercase();
        let ascii = if lower.is_ascii() {
            lower
        } else {
            idn = true;
            format!("xn--{}", punycode(&lower).ok_or(Refusal::Host)?)
        };
        if ascii.starts_with("xn--") {
            idn = true;
        }
        let edge_ok = !ascii.starts_with('-') && !ascii.ends_with('-');
        let chars_ok = ascii
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ascii.len() > 63 || !edge_ok || !chars_ok {
            return Err(Refusal::Host);
        }
        labels.push(ascii);
    }
    let joined = labels.join(".");
    if joined.len() > 253 {
        return Err(Refusal::Host);
    }
    Ok((joined, idn))
}

/// RFC 3492 encoding of one label (the part after `xn--`), or `None` on overflow.
fn punycode(label: &str) -> Option<String> {
    const BASE: u32 = 36;
    const T_MIN: u32 = 1;
    const T_MAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    const DIGITS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let digit =
        |d: u32| -> Option<char> { DIGITS.get(usize::try_from(d).ok()?).map(|b| char::from(*b)) };
    let adapt = |mut delta: u32, points: u32, first: bool| -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / points;
        let mut step = 0;
        while delta > ((BASE - T_MIN) * T_MAX) / 2 {
            delta /= BASE - T_MIN;
            step += BASE;
        }
        step + (BASE - T_MIN + 1) * delta / (delta + SKEW)
    };
    let input: Vec<u32> = label.chars().map(u32::from).collect();
    let mut out: String = label.chars().filter(char::is_ascii).collect();
    let basic = u32::try_from(out.len()).ok()?;
    let mut handled = basic;
    if basic > 0 {
        out.push('-');
    }
    let (mut code, mut delta, mut bias) = (128u32, 0u32, 72u32);
    let total = u32::try_from(input.len()).ok()?;
    while handled < total {
        let next = input.iter().copied().filter(|c| *c >= code).min()?;
        delta = delta.checked_add((next - code).checked_mul(handled + 1)?)?;
        code = next;
        for &c in &input {
            if c < code {
                delta = delta.checked_add(1)?;
            }
            if c == code {
                let mut quotient = delta;
                let mut step = BASE;
                loop {
                    let threshold = if step <= bias {
                        T_MIN
                    } else if step >= bias + T_MAX {
                        T_MAX
                    } else {
                        step - bias
                    };
                    if quotient < threshold {
                        break;
                    }
                    out.push(digit(
                        threshold + (quotient - threshold) % (BASE - threshold),
                    )?);
                    quotient = (quotient - threshold) / (BASE - threshold);
                    step += BASE;
                }
                out.push(digit(quotient)?);
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        code = code.checked_add(1)?;
    }
    Some(out)
}

fn encode(text: &str, keep: &[u8]) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || keep.contains(&b)
        {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// A `mailto` target as RFC 6068 describes: split on the literal `?` and `&`
/// first, then decode each part; keep only the address, `to`, `cc` and
/// `subject`; re-encode what is kept.
fn mail(rest: &str) -> Result<Checked, Refusal> {
    let (address_part, query) = match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    };
    let mut to = Vec::new();
    let mut cc = Vec::new();
    let mut subject = None;
    push_addresses(&mut to, address_part)?;
    for field in query.split('&').filter(|f| !f.is_empty()) {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        let name = decode(name)?.to_ascii_lowercase();
        match name.as_str() {
            "to" => push_addresses(&mut to, value)?,
            "cc" => push_addresses(&mut cc, value)?,
            "subject" => {
                let value = decode(value).map_err(|why| match why {
                    Refusal::Encoding => Refusal::MailValue,
                    other => other,
                })?;
                subject = Some(value);
            }
            // `body`, `bcc`, `attach` and everything else are dropped, and a
            // dropped value is not looked at: only what is kept can refuse.
            _ => {}
        }
    }
    if to.is_empty() {
        return Err(Refusal::Address);
    }
    let list = |addresses: &[String]| -> String {
        addresses
            .iter()
            .map(|a| encode(a, b"@"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut shown = format!("mailto:{}", list(&to));
    let mut sep = '?';
    if !cc.is_empty() {
        shown.push(sep);
        let _ = write!(shown, "cc={}", list(&cc));
        sep = '&';
    }
    if let Some(s) = subject {
        shown.push(sep);
        let _ = write!(shown, "subject={}", encode(&s, b""));
    }
    Ok(Checked {
        kind: Kind::Mail,
        shown,
        host: None,
        idn: false,
        mismatch: false,
    })
}

fn push_addresses(into: &mut Vec<String>, encoded: &str) -> Result<(), Refusal> {
    // A comma separates addresses in the encoded form; decode each on its own.
    for part in encoded.split(',').filter(|p| !p.is_empty()) {
        let address = decode(part).map_err(|why| match why {
            Refusal::Encoding => Refusal::Address,
            other => other,
        })?;
        let bad = address
            .chars()
            .any(|c| matches!(c, '?' | '&') || c.is_control());
        if bad
            || address.matches('@').count() != 1
            || address.starts_with('@')
            || address.ends_with('@')
        {
            return Err(Refusal::Address);
        }
        into.push(address);
    }
    Ok(())
}

/// Whether `visible` looks like a URL: a web scheme, a leading `www.`, or a
/// dotted name with an alphabetic last label and an optional path.
fn looks_like_url(visible: &str) -> bool {
    let text = visible.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) || text.contains('@') {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return true;
    }
    let host = text.split(['/', '?', '#', ':']).next().unwrap_or("");
    let mut labels = host.rsplit('.');
    let tld_ok = labels
        .next()
        .is_some_and(|t| t.chars().count() >= 2 && t.chars().all(char::is_alphabetic));
    tld_ok && host.contains('.') && !host.starts_with('.')
}

/// Whether the visible text looks like a URL and differs from `target`.
fn mismatch(target: &Checked, visible: &str) -> bool {
    if !looks_like_url(visible) {
        return false;
    }
    let text = visible.trim();
    let lower = text.to_ascii_lowercase();
    let candidate = if lower.starts_with("http://") || lower.starts_with("https://") {
        text.to_owned()
    } else {
        // No scheme in the text: compare as the same scheme as the target.
        let scheme = if target.shown.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{text}")
    };
    match parse(&candidate) {
        Ok(Parsed::Link(c)) => {
            let plain = |s: &str| {
                s.strip_suffix('/')
                    .map_or_else(|| s.to_owned(), str::to_owned)
            };
            plain(&c.shown) != plain(&target.shown)
        }
        _ => true,
    }
}

/// What a plain copy of a link gives: its visible text, whatever the target is.
#[must_use]
pub fn copy_plain(visible: &str) -> &str {
    visible
}

/// What "copy link target" gives: the target as the confirmation showed it, and
/// only after that confirmation. `confirmed` must be true only once the
/// terminal has drawn the confirmation for this target and the user accepted it.
/// The same call serves a paste-as-command that takes a link target.
#[must_use]
pub fn copy_target(checked: &Checked, confirmed: bool) -> Option<&str> {
    confirmed.then_some(checked.shown.as_str())
}
