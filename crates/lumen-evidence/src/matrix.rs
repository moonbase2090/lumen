//! Reach mode (criteria 2.1): replay the session matrix and count the sessions
//! that show what the arm specifies.
//!
//! # Session matrix format
//!
//! One JSON file, `{"revision": "..", "sessions": [..]}`. A session is a
//! corpus case (see the crate documentation, including `expect`) with three
//! more fields:
//!
//! - `session`: one of [`SESSION_TYPES`].
//! - `platform`: `linux` or `macos`.
//! - `sealed`: whether the layer's sealing requirements hold in this session.
//!   Arm A replays the session through a sealed or an unsealed layer
//!   accordingly; Arm B ignores it.
//!
//! The matrix has exactly one session for each type on each platform, 14 in
//! all, and every session has an `expect` for both arms. A file that does not
//! fit is refused with the reason, so a missing or doubled cell cannot be
//! counted.
//!
//! A session is reached when the item kinds the arm produced equal its
//! `expect`. This is the item-level oracle: it checks what the byte pipeline
//! hands to the renderer, not the pixels. The screenshot-diff oracle of
//! criterion 2.3 is a separate mode, and the summary says which oracle ran.

use crate::{Arm, Case, HARNESS_VERSION, case_of, run_case};
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// The seven session types of the matrix.
pub const SESSION_TYPES: [&str; 7] = [
    "local-shell",
    "ssh",
    "serial",
    "nested-shell",
    "plain-multiplexer",
    "lumen-multiplexer",
    "alt-screen-app",
];

/// The two platforms of the matrix.
pub const PLATFORMS: [&str; 2] = ["linux", "macos"];

/// Pass threshold for Arm B, in sessions of 14 (criteria 2.1).
pub const ARM_B_THRESHOLD: usize = 12;

/// Which arm a reach run replays. Arm A's layer state comes from each session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachArm {
    /// Arm B.
    B,
    /// Arm A.
    A,
}

impl ReachArm {
    fn expect_key(self) -> &'static str {
        match self {
            ReachArm::B => "b",
            ReachArm::A => "a",
        }
    }
}

/// One session of the matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The scripted run and what each arm must show.
    pub case: Case,
    /// One of [`SESSION_TYPES`].
    pub session: String,
    /// One of [`PLATFORMS`].
    pub platform: String,
    /// The layer's sealing requirements hold (Arm A only).
    pub sealed: bool,
}

/// A loaded session matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    /// The matrix revision.
    pub revision: String,
    /// The 14 sessions, in file order.
    pub sessions: Vec<Session>,
}

fn session_of(v: &Value) -> Result<Session, String> {
    let case = case_of(v)?;
    let id = &case.id;
    let session = v["session"]
        .as_str()
        .filter(|s| SESSION_TYPES.contains(s))
        .ok_or_else(|| format!("{id}: session must be one of {SESSION_TYPES:?}"))?
        .to_owned();
    let platform = v["platform"]
        .as_str()
        .filter(|p| PLATFORMS.contains(p))
        .ok_or_else(|| format!("{id}: platform must be one of {PLATFORMS:?}"))?
        .to_owned();
    let sealed = v["sealed"]
        .as_bool()
        .ok_or_else(|| format!("{id}: sealed must be true or false"))?;
    for key in ["b", "a"] {
        if !case.expect.contains_key(key) {
            return Err(format!("{id}: expect needs a list for {key:?}"));
        }
    }
    Ok(Session {
        case,
        session,
        platform,
        sealed,
    })
}

/// Load and check a session matrix file.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed file, a missing revision,
/// a bad session, a duplicate id, or a matrix that is not exactly one session
/// per type per platform.
pub fn load_matrix(path: &Path) -> Result<Matrix, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let revision = doc["revision"]
        .as_str()
        .ok_or_else(|| format!("{}: no revision", path.display()))?
        .to_owned();
    let sessions = doc["sessions"]
        .as_array()
        .ok_or_else(|| format!("{}: no sessions", path.display()))?
        .iter()
        .map(session_of)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ids = BTreeSet::new();
    let mut cells = BTreeSet::new();
    for s in &sessions {
        if !ids.insert(s.case.id.as_str()) {
            return Err(format!("duplicate session id {}", s.case.id));
        }
        if !cells.insert((s.session.as_str(), s.platform.as_str())) {
            return Err(format!(
                "{}: a second {} session on {}",
                s.case.id, s.session, s.platform
            ));
        }
    }
    let want = SESSION_TYPES.len() * PLATFORMS.len();
    if sessions.len() != want {
        return Err(format!(
            "the matrix has {} sessions; it must have {want}, one per type per platform",
            sessions.len()
        ));
    }
    Ok(Matrix { revision, sessions })
}

/// What one session produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    /// Session id.
    pub id: String,
    /// Session type.
    pub session: String,
    /// Platform.
    pub platform: String,
    /// The item kinds the arm produced.
    pub got: Vec<&'static str>,
    /// The item kinds the session expects.
    pub want: Vec<String>,
}

impl SessionResult {
    /// The arm showed what the session specifies.
    #[must_use]
    pub fn reached(&self) -> bool {
        self.got == self.want
    }
}

/// The result of a reach run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachReport {
    /// One result per session, in matrix order.
    pub sessions: Vec<SessionResult>,
    /// The arm replayed.
    pub arm: ReachArm,
}

impl ReachReport {
    /// Sessions that were reached.
    #[must_use]
    pub fn reached(&self) -> usize {
        self.sessions.iter().filter(|s| s.reached()).count()
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let total = self.sessions.len();
        let reached = self.reached();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} features={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit,
            header.features,
            header.harness,
            header.corpus,
            header.session_matrix,
            header.sinks
        );
        let _ = writeln!(out, "oracle=item-kinds (screenshot oracle not run)");
        let _ = write!(out, "2.1 reach={reached}/{total}");
        if self.arm == ReachArm::B {
            let verdict = if reached >= ARM_B_THRESHOLD {
                "meets"
            } else {
                "below"
            };
            let _ = write!(
                out,
                " ({verdict} the threshold of {ARM_B_THRESHOLD}/{total})"
            );
        } else {
            let _ = write!(out, " (reported, not gated)");
        }
        let _ = writeln!(out);
        for s in &self.sessions {
            let _ = write!(
                out,
                "{} {} {} {}",
                s.id,
                s.session,
                s.platform,
                if s.reached() { "reached" } else { "missed" }
            );
            if !s.reached() {
                let _ = write!(out, " got={:?} want={:?}", s.got, s.want);
            }
            let _ = writeln!(out);
        }
        out
    }
}

/// Replay the matrix through `arm`, writing the run log to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run_reach<W: Write>(
    matrix: &Matrix,
    commit: &str,
    arm: ReachArm,
    log: W,
) -> std::io::Result<(ReachReport, HeaderInfo)> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: match arm {
            ReachArm::B => "arm-b",
            ReachArm::A => "arm-a",
        }
        .to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: matrix.revision.clone(),
        sinks: effects.table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header)?;
    let mut sessions = Vec::new();
    for s in &matrix.sessions {
        let layer_arm = match (arm, s.sealed) {
            (ReachArm::B, _) => Arm::B,
            (ReachArm::A, true) => Arm::ASealed,
            (ReachArm::A, false) => Arm::AUnsealed,
        };
        let result = run_case(&s.case, layer_arm, &mut rec)?;
        sessions.push(SessionResult {
            id: s.case.id.clone(),
            session: s.session.clone(),
            platform: s.platform.clone(),
            got: result.sequence,
            want: s
                .case
                .expect
                .get(arm.expect_key())
                .cloned()
                .unwrap_or_default(),
        });
    }
    rec.finish()?;
    Ok((ReachReport { sessions, arm }, header))
}
