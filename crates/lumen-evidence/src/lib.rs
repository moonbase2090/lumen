//! Replays a corpus through an arm's layer and writes the comparison run log.
//!
//! This crate is test-only: nothing shipped depends on it. It measures what an
//! arm's byte pipeline can show on its own. It draws nothing and has no
//! terminal, so the authority-leak count is reported as not measured.
//!
//! # Corpus format
//!
//! A corpus is a directory of `*.json` files read in name order. Each file is
//! `{"revision": "..", "cases": [..]}`; every file must give the same
//! revision. A case is:
//!
//! - `id`, `category`: strings.
//! - `reads`: the PTY reads in order. Each is `{"hex": ".."}` or
//!   `{"fill": "<hex byte>", "count": n}`, with an optional `"t_ms"` (default 0).
//! - `context`: optional `{"alt_screen": bool, "tmux_or_screen": bool,
//!   "container": bool}`. `container` is whether a command's container is
//!   open; it defaults to true, as while a command runs. With false, Track 0
//!   goes to the background strip.
//! - `forbid`: optional item kinds that must not appear (for example `sealed`
//!   for input that never came from a sealed layer). Each occurrence is a
//!   violation. The kinds are the item kinds of either arm
//!   (`lumen_arm_b::ITEM_KINDS`, `lumen_arm_a::pipeline::ITEM_KINDS`); an
//!   unknown kind fails the load, so a typo cannot pass silently. A kind the
//!   chosen arm never reports (`background` for Arm A) simply never appears.

#![forbid(unsafe_code)]

use lumen_arm_a::pipeline::ITEM_KINDS as ARM_A_KINDS;
use lumen_arm_b::ITEM_KINDS as ARM_B_KINDS;
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// Frames and bytes a producer may use per second in a full budget.
const FULL_FRAMES: usize = 2000;
const FULL_BYTES: usize = 8 << 20;

/// One PTY read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// The bytes read.
    pub bytes: Vec<u8>,
    /// Time of the read in milliseconds.
    pub t_ms: u64,
}

/// One corpus case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Case id.
    pub id: String,
    /// Case category.
    pub category: String,
    /// The reads, in order.
    pub reads: Vec<Read>,
    /// Alternate screen is active.
    pub alt_screen: bool,
    /// `TMUX` or `STY` is set.
    pub tmux_or_screen: bool,
    /// A command's container is open.
    pub container: bool,
    /// Item kinds that must not appear.
    pub forbid: Vec<String>,
}

/// A loaded corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    /// The corpus revision.
    pub revision: String,
    /// All cases, in file then case order.
    pub cases: Vec<Case>,
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err(format!("bad hex {s:?}"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| format!("bad hex {s:?}")))
        .collect()
}

fn read_of(v: &Value) -> Result<Read, String> {
    let bytes = if let Some(h) = v["hex"].as_str() {
        unhex(h)?
    } else if let Some(f) = v["fill"].as_str() {
        let byte = unhex(f)?;
        let [byte] = byte[..] else {
            return Err("fill is one byte".into());
        };
        let count = v["count"].as_u64().ok_or("fill needs a count")?;
        vec![byte; usize::try_from(count).map_err(|_| "count too large")?]
    } else {
        return Err("a read needs hex or fill".into());
    };
    Ok(Read {
        bytes,
        t_ms: v["t_ms"].as_u64().unwrap_or(0),
    })
}

fn case_of(v: &Value) -> Result<Case, String> {
    let id = v["id"].as_str().ok_or("case without id")?.to_owned();
    let reads = v["reads"]
        .as_array()
        .ok_or_else(|| format!("{id}: no reads"))?
        .iter()
        .map(read_of)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{id}: {e}"))?;
    let forbid: Vec<String> = v["forbid"].as_array().map_or_else(Vec::new, |a| {
        a.iter()
            .filter_map(|k| k.as_str().map(str::to_owned))
            .collect()
    });
    if forbid.len() != v["forbid"].as_array().map_or(0, Vec::len) {
        return Err(format!("{id}: forbid entries must be strings"));
    }
    let known = |k: &str| ARM_B_KINDS.contains(&k) || ARM_A_KINDS.contains(&k);
    if let Some(bad) = forbid.iter().find(|k| !known(k)) {
        return Err(format!(
            "{id}: unknown forbid kind {bad:?}; known kinds are {ARM_B_KINDS:?} and {ARM_A_KINDS:?}"
        ));
    }
    Ok(Case {
        category: v["category"].as_str().unwrap_or_default().to_owned(),
        alt_screen: v["context"]["alt_screen"].as_bool().unwrap_or(false),
        tmux_or_screen: v["context"]["tmux_or_screen"].as_bool().unwrap_or(false),
        container: v["context"]["container"].as_bool().unwrap_or(true),
        forbid: v["forbid"].as_array().map_or_else(Vec::new, |a| {
            a.iter()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect()
        }),
        reads,
        id,
    })
}

/// Load every `*.json` file in `dir`, in name order.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed file, a missing or
/// disagreeing revision, a duplicate case id, or an empty corpus.
pub fn load_corpus(dir: &Path) -> Result<Corpus, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut revision: Option<String> = None;
    let mut cases: Vec<Case> = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let doc: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        let rev = doc["revision"]
            .as_str()
            .ok_or_else(|| format!("{}: no revision", f.display()))?;
        match &revision {
            Some(r) if r != rev => {
                return Err(format!("{}: revision {rev} differs from {r}", f.display()));
            }
            _ => revision = Some(rev.to_owned()),
        }
        for c in doc["cases"].as_array().map_or(&[][..], Vec::as_slice) {
            let case = case_of(c).map_err(|e| format!("{}: {e}", f.display()))?;
            if cases.iter().any(|x| x.id == case.id) {
                return Err(format!("duplicate case id {}", case.id));
            }
            cases.push(case);
        }
    }
    let revision = revision.ok_or("no corpus files")?;
    if cases.is_empty() {
        return Err("corpus has no cases".into());
    }
    Ok(Corpus { revision, cases })
}

/// What one case produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// Case id.
    pub id: String,
    /// Items by kind.
    pub kinds: BTreeMap<&'static str, usize>,
    /// The layer's counters after the stream ended, as the arm prints them.
    pub counters: String,
    /// How many forbidden items appeared.
    pub violations: usize,
}

/// The result of a whole run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One result per case, in corpus order.
    pub cases: Vec<CaseResult>,
    /// The authority-leak line: not measured while no terminal exists.
    pub authority: String,
}

impl Report {
    /// Total forbidden items across all cases.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.cases.iter().map(|c| c.violations).sum()
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} features={} corpus={} sinks={}",
            header.commit, header.features, header.corpus, header.sinks
        );
        let _ = writeln!(
            out,
            "cases={} violations={}",
            self.cases.len(),
            self.violations()
        );
        let _ = write!(out, "{}", self.authority);
        for c in &self.cases {
            let _ = writeln!(
                out,
                "{} items={:?} violations={} counters={}",
                c.id, c.kinds, c.violations, c.counters
            );
        }
        out
    }
}

/// Which arm, and for Arm A which kind of layer, the corpus is replayed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// Arm B: Track 0 everywhere plus the sealed upgrade.
    B,
    /// Arm A on a layer whose sealing requirements all hold.
    ASealed,
    /// Arm A on a layer where a sealing requirement failed, so there are no keys.
    AUnsealed,
}

impl Arm {
    /// The name written into the log header.
    #[must_use]
    pub fn features(self) -> &'static str {
        match self {
            Arm::B => "arm-b",
            Arm::ASealed => "arm-a sealed-layer",
            Arm::AUnsealed => "arm-a unsealed-layer",
        }
    }
}

/// One arm's layer behind the calls the driver needs.
enum AnyLayer {
    A(lumen_arm_a::pipeline::Layer),
    B(lumen_arm_b::Layer),
}

impl AnyLayer {
    fn new(arm: Arm) -> Self {
        match arm {
            Arm::B => AnyLayer::B(lumen_arm_b::Layer::new(lumen_arm_b::Config {
                budget: lumen_arm_b::Budget::quarter_of(FULL_FRAMES, FULL_BYTES),
                background_as_text: false,
            })),
            Arm::ASealed | Arm::AUnsealed => {
                let state = if arm == Arm::ASealed {
                    lumen_arm_a::LayerState::Sealed
                } else {
                    lumen_arm_a::LayerState::Unsealed
                };
                AnyLayer::A(lumen_arm_a::pipeline::Layer::new(
                    lumen_arm_a::pipeline::Config {
                        budget: lumen_arm_a::pipeline::Budget::quarter_of(FULL_FRAMES, FULL_BYTES),
                    },
                    state,
                ))
            }
        }
    }

    fn open_container(&mut self) {
        match self {
            AnyLayer::A(l) => l.open_container(),
            AnyLayer::B(l) => l.open_container(),
        }
    }

    /// Feed one read and return the `(kind, len)` summary of what came out.
    fn feed(&mut self, bytes: &[u8], read: &Read, case: &Case) -> Vec<(&'static str, usize)> {
        match self {
            AnyLayer::A(l) => l
                .feed(
                    bytes,
                    lumen_arm_a::pipeline::Context {
                        alt_screen: case.alt_screen,
                        tmux_or_screen: case.tmux_or_screen,
                        now_ms: read.t_ms,
                    },
                )
                .iter()
                .map(lumen_arm_a::pipeline::item_summary)
                .collect(),
            AnyLayer::B(l) => l
                .feed(
                    bytes,
                    lumen_arm_b::Context {
                        alt_screen: case.alt_screen,
                        tmux_or_screen: case.tmux_or_screen,
                        now_ms: read.t_ms,
                    },
                )
                .iter()
                .map(lumen_arm_b::item_summary)
                .collect(),
        }
    }

    fn finish(&mut self) -> Vec<(&'static str, usize)> {
        match self {
            AnyLayer::A(l) => l
                .finish()
                .iter()
                .map(lumen_arm_a::pipeline::item_summary)
                .collect(),
            AnyLayer::B(l) => l.finish().iter().map(lumen_arm_b::item_summary).collect(),
        }
    }

    /// The counters in the form both arms print: the same field names.
    fn counters(&self) -> String {
        match self {
            AnyLayer::A(l) => format!("{:?}", l.counters()),
            AnyLayer::B(l) => format!("{:?}", l.counters()),
        }
    }
}

fn run_case<W: Write>(case: &Case, arm: Arm, rec: &mut Recorder<W>) -> std::io::Result<CaseResult> {
    let mut layer = AnyLayer::new(arm);
    if case.container {
        layer.open_container();
    }
    rec.case(&case.id)?;
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut violations = 0;
    let mut last_t = 0;
    let mut record =
        |t: u64, bytes_in: usize, summary: &[(&'static str, usize)], rec: &mut Recorder<W>| {
            for (kind, _) in summary {
                *kinds.entry(kind).or_insert(0) += 1;
                if case.forbid.iter().any(|f| f == kind) {
                    violations += 1;
                }
            }
            rec.read(t, bytes_in, summary)
        };
    for r in &case.reads {
        last_t = r.t_ms;
        let summary = layer.feed(&r.bytes, r, case);
        record(r.t_ms, r.bytes.len(), &summary, rec)?;
    }
    let summary = layer.finish();
    record(last_t, 0, &summary, rec)?;
    Ok(CaseResult {
        id: case.id.clone(),
        kinds,
        counters: layer.counters(),
        violations,
    })
}

/// Replay the corpus, writing the run log to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run<W: Write>(
    corpus: &Corpus,
    commit: &str,
    log: W,
) -> std::io::Result<(Report, HeaderInfo)> {
    run_arm(corpus, commit, Arm::B, log)
}

/// Replay the corpus through `arm`, writing the run log to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run_arm<W: Write>(
    corpus: &Corpus,
    commit: &str,
    arm: Arm,
    log: W,
) -> std::io::Result<(Report, HeaderInfo)> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: arm.features().to_owned(),
        corpus: corpus.revision.clone(),
        sinks: effects.table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header)?;
    let mut cases = Vec::new();
    for case in &corpus.cases {
        cases.push(run_case(case, arm, &mut rec)?);
    }
    rec.finish()?;
    Ok((
        Report {
            cases,
            authority: effects.report(),
        },
        header,
    ))
}
