//! Parity mode (criteria 2.9): count the Lumen channels that accept an action
//! a PTY write could not already perform.
//!
//! The harness cannot discover channels. The arm's author declares them in a
//! channel catalogue, and the harness checks the catalogue and counts. The
//! count is therefore as complete as the catalogue, and a reviewer must check
//! the catalogue against the code at the measured commit. A catalogue with no
//! channel is reported as not measured, not as a pass: a count of zero over
//! nothing says nothing about the gate.
//!
//! # Catalogue format
//!
//! One JSON file, `{"revision": "..", "channels": [..]}`. A channel is
//! `{"id": "..", "actions": [..]}`. An action is `{"id": "..",
//! "pty_equivalent": ".."}`, where `pty_equivalent` says in a sentence which
//! PTY write already performs the same action, or is `null` when none does.
//! A `null` makes the channel one that accepts more than a PTY write could.
//! Channel ids are unique, and action ids are unique within a channel.

use crate::HARNESS_VERSION;
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// Exit code: channels were declared and none accepts more than a PTY write.
pub const EXIT_ZERO: u8 = 0;
/// Exit code: at least one channel accepts more than a PTY write could.
pub const EXIT_MORE: u8 = 1;
/// Exit code: no channel was declared, so nothing was measured. Code 2 is
/// left to usage and load errors.
pub const EXIT_NOT_MEASURED: u8 = 3;

/// One action a channel accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Action id.
    pub id: String,
    /// The PTY write that already does the same, if there is one.
    pub pty_equivalent: Option<String>,
}

/// One channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// Channel id.
    pub id: String,
    /// The actions it accepts.
    pub actions: Vec<Action>,
}

/// A loaded catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    /// The catalogue revision.
    pub revision: String,
    /// The declared channels.
    pub channels: Vec<Channel>,
}

fn action_of(channel: &str, v: &Value) -> Result<Action, String> {
    let id = v["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{channel}: an action needs an id"))?
        .to_owned();
    let pty_equivalent = match &v["pty_equivalent"] {
        Value::Null => None,
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        _ => {
            return Err(format!(
                "{channel}/{id}: pty_equivalent must be a sentence or null"
            ));
        }
    };
    Ok(Action { id, pty_equivalent })
}

fn channel_of(v: &Value) -> Result<Channel, String> {
    let id = v["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("a channel needs an id")?
        .to_owned();
    let actions = v["actions"]
        .as_array()
        .ok_or_else(|| format!("{id}: no actions list"))?
        .iter()
        .map(|a| action_of(&id, a))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    if let Some(dup) = actions.iter().find(|a| !seen.insert(a.id.as_str())) {
        return Err(format!("{id}: duplicate action id {}", dup.id));
    }
    Ok(Channel { id, actions })
}

/// Load and check a channel catalogue.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed file, a missing revision,
/// a bad channel or action, or a duplicate id.
pub fn load_catalogue(path: &Path) -> Result<Catalogue, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let revision = doc["revision"]
        .as_str()
        .ok_or_else(|| format!("{}: no revision", path.display()))?
        .to_owned();
    let channels = doc["channels"]
        .as_array()
        .ok_or_else(|| format!("{}: no channels list", path.display()))?
        .iter()
        .map(channel_of)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut seen = BTreeSet::new();
    if let Some(dup) = channels.iter().find(|c| !seen.insert(c.id.as_str())) {
        return Err(format!("duplicate channel id {}", dup.id));
    }
    Ok(Catalogue { revision, channels })
}

/// The result of a parity run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityReport {
    /// The catalogue revision.
    pub revision: String,
    /// Channels declared.
    pub channels: usize,
    /// Actions declared.
    pub actions: usize,
    /// Channels with at least one action that has no PTY equivalent, with those actions.
    pub accepting_more: Vec<(String, Vec<String>)>,
}

impl ParityReport {
    /// The criteria 2.9 count: channels that accept more than a PTY write could.
    #[must_use]
    pub fn count(&self) -> usize {
        self.accepting_more.len()
    }

    /// Nothing was declared, so nothing was measured.
    #[must_use]
    pub fn measured(&self) -> bool {
        self.channels > 0
    }

    /// The process exit code: [`EXIT_ZERO`] when channels were declared and
    /// none accepts more than a PTY write, [`EXIT_MORE`] when one does, and
    /// [`EXIT_NOT_MEASURED`] when no channel was declared. Only the first is
    /// a pass, so a gate script cannot read an empty catalogue as one.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if !self.measured() {
            EXIT_NOT_MEASURED
        } else if self.count() == 0 {
            EXIT_ZERO
        } else {
            EXIT_MORE
        }
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit, header.harness, header.corpus, header.session_matrix, header.sinks
        );
        let _ = writeln!(
            out,
            "channel_catalogue={} channels={} actions={}",
            self.revision, self.channels, self.actions
        );
        if self.measured() {
            let _ = writeln!(
                out,
                "2.9 channels_accepting_more_than_pty={} (threshold 0; the catalogue is a declaration checked by the reviewer)",
                self.count()
            );
        } else {
            let _ = writeln!(
                out,
                "2.9=not measured (no channel is declared at this commit)"
            );
        }
        for (channel, actions) in &self.accepting_more {
            let _ = writeln!(out, "{channel} accepts more: {actions:?}");
        }
        out
    }
}

/// Count the catalogue and write the header-only run log to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run_parity<W: Write>(
    catalogue: &Catalogue,
    commit: &str,
    log: W,
) -> std::io::Result<(ParityReport, HeaderInfo)> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: "arm-a arm-b".to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    Recorder::new(log, &header)?.finish()?;
    let accepting_more = catalogue
        .channels
        .iter()
        .filter_map(|c| {
            let more: Vec<String> = c
                .actions
                .iter()
                .filter(|a| a.pty_equivalent.is_none())
                .map(|a| a.id.clone())
                .collect();
            (!more.is_empty()).then(|| (c.id.clone(), more))
        })
        .collect();
    Ok((
        ParityReport {
            revision: catalogue.revision.clone(),
            channels: catalogue.channels.len(),
            actions: catalogue.channels.iter().map(|c| c.actions.len()).sum(),
            accepting_more,
        },
        header,
    ))
}
