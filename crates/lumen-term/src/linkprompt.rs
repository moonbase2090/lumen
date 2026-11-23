//! What the terminal asks before a link does anything, and how a link is opened.
//!
//! A [`Prompt`] is built from a link found on the grid ([`crate::linkhit::Hit`])
//! after [`crate::link::check`] has judged its target. The window draws it as
//! its own surface, above everything a program can draw, and shows the whole
//! decoded target. Nothing opens until the user answers it.

use crate::link::{self, Checked, Refusal, Verdict};
use crate::linkhit::{Hit, Producer};
use lumen_track0::{is_bidi, is_default_ignorable};
use std::fmt::Write as _;

/// What the prompt is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// A target that may be opened after a yes.
    Confirm(Checked),
    /// A `file` link: shown as inert text, never opened.
    Inert {
        /// The decoded path.
        path: String,
    },
    /// A target that is not acted on.
    Refused(Refusal),
}

/// What a yes to the question does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Hand the target to the system's opener.
    Open,
    /// Put the checked target on the clipboard. The question shows the same
    /// target the copy carries.
    Copy,
}

/// A question the terminal draws about one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Who made the link.
    pub producer: Producer,
    /// The text the link showed in the grid.
    pub visible: String,
    /// What the check said about the target.
    pub body: Body,
    /// What a yes does.
    pub purpose: Purpose,
}

impl Prompt {
    /// The prompt for `hit`, asking whether to open it.
    #[must_use]
    pub fn for_hit(hit: &Hit) -> Self {
        Self::for_purpose(hit, Purpose::Open)
    }

    /// The prompt for `hit`, asking whether to copy its address.
    #[must_use]
    pub fn for_copy(hit: &Hit) -> Self {
        Self::for_purpose(hit, Purpose::Copy)
    }

    fn for_purpose(hit: &Hit, purpose: Purpose) -> Self {
        let body = match link::check(&hit.target, &hit.visible) {
            Verdict::Confirm(checked) => Body::Confirm(checked),
            Verdict::Inert { path } => Body::Inert { path },
            Verdict::Refused(why) => Body::Refused(why),
        };
        Self {
            producer: hit.producer,
            visible: hit.visible.clone(),
            body,
            purpose,
        }
    }

    /// The label of the button that says yes.
    #[must_use]
    pub fn yes_label(&self) -> &'static str {
        match self.purpose {
            Purpose::Open => "Open",
            Purpose::Copy => "Copy",
        }
    }

    /// What a yes puts on the clipboard: the checked target of a copy
    /// question, as the question showed it, and nothing for any other
    /// question. Call it only once the user has said yes.
    #[must_use]
    pub fn copy_text(&self) -> Option<&str> {
        match (&self.body, self.purpose) {
            (Body::Confirm(checked), Purpose::Copy) => link::copy_target(checked, true),
            _ => None,
        }
    }

    /// The target to open if the user says yes, or `None` for a prompt that
    /// only informs.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        match &self.body {
            Body::Confirm(checked) => Some(&checked.shown),
            _ => None,
        }
    }

    /// The lines the prompt shows, before they are wrapped to the box. The
    /// target is one line here and is wrapped, never cut.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        match &self.body {
            Body::Confirm(checked) => {
                out.push(
                    match self.purpose {
                        Purpose::Open => "Open this link?",
                        Purpose::Copy => "Copy this address?",
                    }
                    .to_owned(),
                );
                out.push(String::new());
                out.push(checked.shown.clone());
                out.push(String::new());
                out.push(self.producer_line().to_owned());
                if self.producer == Producer::Program {
                    out.push(format!("Text shown: {}", escapes(&self.visible)));
                }
                if let Some(host) = &checked.host {
                    out.push(format!("Host: {host}"));
                }
                if checked.idn {
                    out.push(
                        "Warning: the host is not plain ASCII; it is shown as it will be used."
                            .to_owned(),
                    );
                }
                if checked.mismatch {
                    out.push(
                        "Warning: the text shown looks like an address that is not this one."
                            .to_owned(),
                    );
                }
            }
            Body::Inert { path } => {
                out.push("This is a file link. It is not opened.".to_owned());
                out.push(String::new());
                out.push(format!("Path: {path}"));
                out.push(String::new());
                out.push(self.producer_line().to_owned());
            }
            Body::Refused(why) => {
                out.push("This link is not opened.".to_owned());
                out.push(String::new());
                out.push(refusal_text(*why).to_owned());
                out.push(String::new());
                out.push(self.producer_line().to_owned());
            }
        }
        out
    }

    fn producer_line(&self) -> &'static str {
        match self.producer {
            Producer::Program => "Set by the program. The text it shows can say anything.",
            Producer::Detected => "Found in the program's output.",
        }
    }
}

/// Every control, bidirectional and default-ignorable code point as `⟨U+XXXX⟩`,
/// so a label cannot hide or reorder anything in the prompt.
#[must_use]
pub fn escapes(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() || is_bidi(c) || is_default_ignorable(c) {
            let _ = write!(out, "\u{27e8}U+{:04X}\u{27e9}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

fn refusal_text(why: Refusal) -> &'static str {
    match why {
        Refusal::Size => "The target is empty or too long.",
        Refusal::Control => "The target has a control character or a space.",
        Refusal::Invisible => "The target has a character that could hide part of it.",
        Refusal::Scheme => "Only https, http and mailto links are opened.",
        Refusal::Encoding => "The target does not decode cleanly.",
        Refusal::Host => "The target has no valid host.",
        Refusal::Port => "The target has a port that is not valid.",
        Refusal::UserInfo => "The target has a user name or password in it.",
        Refusal::Address => "The mail address is not valid.",
        Refusal::MailValue => "A mail field has a control character in it.",
    }
}

/// How a confirmed target is handed to the system.
pub trait LinkOpener {
    /// Open `target`, which the user has confirmed.
    ///
    /// # Errors
    ///
    /// Returns why the system could not be asked.
    fn open(&mut self, target: &str) -> std::io::Result<()>;
}

/// Hands a target to the system's own opener: `open` on macOS, `xdg-open`
/// elsewhere. The target is one argument and is never run through a shell.
pub struct SystemOpener;

impl LinkOpener for SystemOpener {
    fn open(&mut self, target: &str) -> std::io::Result<()> {
        let program = if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else if cfg!(windows) {
            "explorer.exe"
        } else {
            "xdg-open"
        };
        let mut child = std::process::Command::new(program)
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        // Collect the opener when it ends, so it does not stay a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(target: &str, visible: &str, producer: Producer) -> Hit {
        Hit {
            target: target.to_owned(),
            visible: visible.to_owned(),
            producer,
            columns: 0..visible.chars().count(),
        }
    }

    #[test]
    fn a_web_link_asks_with_the_whole_target() {
        let p = Prompt::for_hit(&hit(
            "https://example.com/a/b?c=d",
            "docs",
            Producer::Program,
        ));
        assert_eq!(p.target(), Some("https://example.com/a/b?c=d"));
        let text = p.lines().join("\n");
        assert!(text.contains("https://example.com/a/b?c=d"));
        assert!(text.contains("Set by the program"));
        assert!(text.contains("Text shown: docs"));
    }

    #[test]
    fn text_that_looks_like_another_address_is_flagged() {
        let p = Prompt::for_hit(&hit(
            "https://evil.test/",
            "https://bank.test/",
            Producer::Program,
        ));
        assert!(p.lines().join("\n").contains("Warning: the text shown"));
    }

    #[test]
    fn other_schemes_and_file_links_are_never_openable() {
        let js = Prompt::for_hit(&hit("javascript:alert(1)", "x", Producer::Program));
        assert_eq!(js.target(), None);
        let file = Prompt::for_hit(&hit("file:///etc/hosts", "x", Producer::Program));
        assert_eq!(file.target(), None);
        assert!(file.lines().join("\n").contains("Path: /etc/hosts"));
    }

    #[test]
    fn a_zero_width_character_in_the_text_is_shown_as_an_escape() {
        assert_eq!(escapes("a\u{200b}b"), "a\u{27e8}U+200B\u{27e9}b");
    }
}
