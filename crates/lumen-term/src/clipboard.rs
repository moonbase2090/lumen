//! Reading the system clipboard's text, for the Paste action.
//!
//! The terminal asks the operating system's own clipboard tool for the text, as
//! it does for notifications: `pbpaste` on macOS, and `wl-paste`, `xclip` or
//! `xsel` elsewhere (the first one found). Only text is read, with a size
//! limit, and a missing tool or an empty clipboard is no text.

use std::io::Read;
use std::process::{Command, Stdio};

/// The most clipboard text a paste takes, in bytes.
pub const MAX_PASTE_BYTES: usize = 1 << 20;

/// Where pasted text comes from.
pub trait Clipboard {
    /// The clipboard's text, or `None` if there is none.
    fn text(&self) -> Option<String>;
}

/// The operating system's clipboard.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboard;

fn candidates() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("/usr/bin/pbpaste", &[])]
    } else {
        &[
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-o", "-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--output"]),
        ]
    }
}

impl Clipboard for SystemClipboard {
    fn text(&self) -> Option<String> {
        for (program, args) in candidates() {
            let Ok(mut child) = Command::new(program)
                .args(*args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            let mut bytes = Vec::new();
            if let Some(out) = child.stdout.take() {
                let limit = u64::try_from(MAX_PASTE_BYTES).unwrap_or(u64::MAX);
                let _ = out.take(limit).read_to_end(&mut bytes);
            }
            let _ = child.kill();
            let _ = child.wait();
            if let Ok(text) = String::from_utf8(bytes)
                && !text.is_empty()
            {
                return Some(text);
            }
        }
        None
    }
}
