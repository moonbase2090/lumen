//! What a copy carries.
//!
//! A copy is the logical code points of the visible runs, in order, not the
//! shaped glyphs. A run that is not visible never reaches it. In command and
//! code text the bidirectional controls are stripped, and in command text the
//! zero-width and invisible characters too, and the number stripped is
//! returned so the terminal can show it. Paragraph and heading text keeps its
//! original code points. A copy that spans producers keeps the split: the
//! parts are returned with their producers so the terminal can show it.

/// What kind of text a run is, which decides what a copy strips from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Paragraph or heading text: nothing is stripped.
    Text,
    /// Code: bidirectional controls are stripped.
    Code,
    /// Command text: bidirectional controls and invisible characters are stripped.
    Command,
}

/// One run of text as the copy sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The run's code points in logical order.
    pub text: String,
    /// What kind of text it is.
    pub kind: Kind,
    /// Whether it is visible (size, contrast, clipping, section and coverage).
    pub visible: bool,
    /// Who produced it: a producer id the terminal assigned.
    pub producer: u32,
}

/// A stretch of the copy from one producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The producer.
    pub producer: u32,
    /// Its text, after stripping.
    pub text: String,
}

/// The result of a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// What goes on the clipboard.
    pub text: String,
    /// How many code points were stripped, to be shown.
    pub stripped: usize,
    /// How many runs were hidden and left out.
    pub hidden: usize,
    /// The copy by producer, consecutive runs of one producer merged. More than
    /// one part means the copy spans producers and the terminal shows the split.
    pub parts: Vec<Part>,
}

impl Copied {
    /// Whether the copy spans producers.
    #[must_use]
    pub fn spans_producers(&self) -> bool {
        self.parts.len() > 1
    }
}

/// Bidirectional controls: overrides, isolates and marks.
fn is_bidi_control(c: char) -> bool {
    matches!(u32::from(c), 0x202a..=0x202e | 0x2066..=0x2069 | 0x200e | 0x200f | 0x061c)
}

/// Zero-width and invisible characters, and tag characters, stripped from
/// command text only.
fn is_invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0x200b..=0x200d | 0x2060..=0x2064 | 0xfeff | 0xe0000..=0xe007f
    )
}

/// Strip what `kind` strips from `text`, returning the rest and the count.
#[must_use]
pub fn strip(text: &str, kind: Kind) -> (String, usize) {
    if kind == Kind::Text {
        return (text.to_owned(), 0);
    }
    let mut kept = String::with_capacity(text.len());
    let mut stripped = 0;
    for c in text.chars() {
        if is_bidi_control(c) || (kind == Kind::Command && is_invisible(c)) {
            stripped += 1;
        } else {
            kept.push(c);
        }
    }
    (kept, stripped)
}

/// Copy `runs`, in order.
#[must_use]
pub fn copy(runs: &[Run]) -> Copied {
    let mut out = Copied {
        text: String::new(),
        stripped: 0,
        hidden: 0,
        parts: Vec::new(),
    };
    for run in runs {
        if !run.visible {
            out.hidden += 1;
            continue;
        }
        let (text, stripped) = strip(&run.text, run.kind);
        out.stripped += stripped;
        out.text.push_str(&text);
        match out.parts.last_mut() {
            Some(part) if part.producer == run.producer => part.text.push_str(&text),
            _ => out.parts.push(Part {
                producer: run.producer,
                text,
            }),
        }
    }
    out
}
