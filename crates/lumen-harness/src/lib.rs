//! Shared recording helpers for the phase 0.5 comparison.
//!
//! Both arms record their runs the same way so the comparison is like-for-like.
//! This crate is test-only: no shipped crate depends on it, and it holds no arm
//! logic. It depends on neither arm. An arm describes what it produced as
//! plain `(kind, len)` pairs.
//!
//! - [`Recorder`] writes JSON lines: one header line, then one line per read.
//!   Lines carry kinds and lengths only, never payload text.
//! - [`EffectLog`] is the contract for the authority-leak count. The terminal's
//!   effect sinks call it. It reports a zero only when a sink was exercised,
//!   and it never turns a stub-level run into a measurement.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, Write};

/// Version of this recording format. Written into the header line.
pub const FORMAT_VERSION: u32 = 1;

/// Facts about a run that every log starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderInfo {
    /// The commit the arm was built from.
    pub commit: String,
    /// The cargo features the build used.
    pub features: String,
    /// The corpus revision the run replays.
    pub corpus: String,
}

/// Write `s` as a JSON string, escaping what JSON requires.
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(out, format_args!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Writes the JSON-lines run log.
#[derive(Debug)]
pub struct Recorder<W: Write> {
    out: W,
}

impl<W: Write> Recorder<W> {
    /// A recorder that writes the header line first.
    ///
    /// # Errors
    ///
    /// Returns the write error, if any.
    pub fn new(mut out: W, header: &HeaderInfo) -> io::Result<Self> {
        let mut line = String::from("{\"header\":{\"format\":");
        line.push_str(&FORMAT_VERSION.to_string());
        for (key, value) in [
            ("commit", &header.commit),
            ("features", &header.features),
            ("corpus", &header.corpus),
        ] {
            line.push_str(",\"");
            line.push_str(key);
            line.push_str("\":");
            json_string(&mut line, value);
        }
        line.push_str("}}\n");
        out.write_all(line.as_bytes())?;
        Ok(Self { out })
    }

    /// Record one read: its time, the bytes that went in, and what came out as
    /// `(kind, len)` pairs. The pairs carry no content.
    ///
    /// # Errors
    ///
    /// Returns the write error, if any.
    pub fn read(&mut self, t_ms: u64, bytes_in: usize, items: &[(&str, usize)]) -> io::Result<()> {
        let mut line = String::new();
        let _ = write!(
            line,
            "{{\"t_ms\":{t_ms},\"bytes_in\":{bytes_in},\"items\":["
        );
        for (i, (kind, len)) in items.iter().enumerate() {
            if i > 0 {
                line.push(',');
            }
            line.push_str("{\"kind\":");
            json_string(&mut line, kind);
            {
                let _ = write!(line, ",\"len\":{len}}}");
            };
        }
        line.push_str("]}\n");
        self.out.write_all(line.as_bytes())
    }

    /// Flush and give the writer back.
    ///
    /// # Errors
    ///
    /// Returns the flush error, if any.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// The effects the criteria count (section 2.4). Each is a sink in the
/// terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    /// A clipboard write.
    ClipboardWrite,
    /// A notification.
    Notification,
    /// A link open.
    LinkOpen,
    /// A capability grant.
    Grant,
    /// An exit status shown for a command.
    ExitStatus,
    /// A command-boundary change.
    BoundaryChange,
}

impl Effect {
    /// Every effect, in a fixed order.
    pub const ALL: [Effect; 6] = [
        Effect::ClipboardWrite,
        Effect::Notification,
        Effect::LinkOpen,
        Effect::Grant,
        Effect::ExitStatus,
        Effect::BoundaryChange,
    ];

    /// A stable name for logs.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Effect::ClipboardWrite => "clipboard_write",
            Effect::Notification => "notification",
            Effect::LinkOpen => "link_open",
            Effect::Grant => "grant",
            Effect::ExitStatus => "exit_status",
            Effect::BoundaryChange => "boundary_change",
        }
    }

    fn index(self) -> usize {
        Effect::ALL.iter().position(|e| *e == self).unwrap_or(0)
    }
}

/// Where the input that reached a sink came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A sealed, verified frame.
    Sealed,
    /// Anything unsealed: Track 0, plain output, a paste, a program's bytes.
    Unsealed,
}

/// What kind of run is being logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The sinks are stubs driven by the harness. Nothing about a real terminal
    /// is measured.
    Stub,
    /// The sinks are the terminal's own effect code.
    Terminal,
}

/// An unsealed-origin attempt at a sink: a leak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    /// The sink.
    pub effect: Effect,
    /// An identifier of the input that caused it, from the corpus.
    pub input: String,
}

/// The outcome for criterion 2.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not measured, with the reason. Never a pass.
    NotMeasured(String),
    /// Every sink was exercised and no unsealed input reached any of them.
    Zero,
    /// At least one unsealed input reached a sink.
    NonZero(usize),
}

/// One line of the per-sink report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkLine {
    /// The sink.
    pub effect: Effect,
    /// How many times the sink was reached by any input.
    pub exercised: u64,
    /// How many of those were unsealed-origin.
    pub unsealed: u64,
}

/// Records attempts at the terminal's effect sinks.
///
/// The terminal's sink code calls [`attempt`](Self::attempt) at the entry of
/// each sink, with the origin of the input that caused it. A sink that was never
/// reached has `exercised == 0`, and a zero for it means nothing.
#[derive(Debug)]
pub struct EffectLog {
    level: Level,
    exercised: [u64; 6],
    unsealed: [u64; 6],
    leaks: Vec<Leak>,
}

impl EffectLog {
    /// An empty log for a run at `level`.
    #[must_use]
    pub fn new(level: Level) -> Self {
        Self {
            level,
            exercised: [0; 6],
            unsealed: [0; 6],
            leaks: Vec::new(),
        }
    }

    /// Record that `effect` was reached by an input from `origin`. `input` names
    /// the corpus input.
    pub fn attempt(&mut self, effect: Effect, origin: Origin, input: &str) {
        let i = effect.index();
        self.exercised[i] = self.exercised[i].saturating_add(1);
        if origin == Origin::Unsealed {
            self.unsealed[i] = self.unsealed[i].saturating_add(1);
            self.leaks.push(Leak {
                effect,
                input: input.to_owned(),
            });
        }
    }

    /// Every unsealed-origin attempt, in order.
    #[must_use]
    pub fn leaks(&self) -> &[Leak] {
        &self.leaks
    }

    /// One line per sink with its exercised count. A result must print these.
    #[must_use]
    pub fn sinks(&self) -> Vec<SinkLine> {
        Effect::ALL
            .iter()
            .map(|&effect| SinkLine {
                effect,
                exercised: self.exercised[effect.index()],
                unsealed: self.unsealed[effect.index()],
            })
            .collect()
    }

    /// The verdict for criterion 2.4.
    ///
    /// A stub-level run is always not measured. At terminal level, a zero needs
    /// every sink exercised; a sink that was never reached makes the result not
    /// measured. Any unsealed-origin attempt is non-zero whatever else holds.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let leaks = self.leaks.len();
        if leaks > 0 && self.level == Level::Terminal {
            return Verdict::NonZero(leaks);
        }
        if self.level == Level::Stub {
            return Verdict::NotMeasured(
                "stub-level run: the sinks are not the terminal's own effect code".to_owned(),
            );
        }
        let idle: Vec<&str> = self
            .sinks()
            .iter()
            .filter(|s| s.exercised == 0)
            .map(|s| s.effect.name())
            .collect();
        if idle.is_empty() {
            Verdict::Zero
        } else {
            Verdict::NotMeasured(format!("sinks never exercised: {}", idle.join(", ")))
        }
    }

    /// A printable report: the verdict and every sink's exercised count.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        let level = match self.level {
            Level::Stub => "stub",
            Level::Terminal => "terminal",
        };
        {
            let _ = writeln!(out, "level={level}");
        };
        match self.verdict() {
            Verdict::NotMeasured(why) => {
                let _ = writeln!(out, "2.4=not measured ({why})");
            }
            Verdict::Zero => out.push_str("2.4=zero\n"),
            Verdict::NonZero(n) => {
                let _ = writeln!(out, "2.4=non-zero ({n})");
            }
        }
        for s in self.sinks() {
            {
                let _ = writeln!(
                    out,
                    "{} exercised={} unsealed={}",
                    s.effect.name(),
                    s.exercised,
                    s.unsealed
                );
            };
        }
        out
    }
}
