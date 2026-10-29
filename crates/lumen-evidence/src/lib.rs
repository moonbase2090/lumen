//! Replays a corpus through an arm's layer and writes the comparison run log.
//!
//! This crate is test-only: nothing shipped depends on it. It measures what an
//! arm's byte pipeline can show on its own, and it draws nothing. The ordinary
//! output a layer passes on is fed to the upstream emulator through
//! `lumen-sinks`, so the authority-leak count is read from the effects that
//! emulator raises. Sinks that have no code in this tree are never exercised,
//! and the count stays "not measured" until every sink is.
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
//! - `expect`: optional `{"b": [..], "a": [..]}`, the item kinds the arm must
//!   produce, in order, once adjacent `output` items and adjacent `text` items
//!   are each joined (a read boundary can split them). Used by the reach mode
//!   ([`matrix`]); the corpus replay ignores it.
//! - `forbid`: optional item kinds that must not appear (for example `sealed`
//!   for input that never came from a sealed layer). Each occurrence is a
//!   violation. The kinds are the item kinds of either arm
//!   (`lumen_arm_b::ITEM_KINDS`, `lumen_arm_a::pipeline::ITEM_KINDS`); an
//!   unknown kind fails the load, so a typo cannot pass silently. A kind the
//!   chosen arm never reports (`background` for Arm A) simply never appears.
//!
//! Every replayed layer holds the fixed test keys [`TEST_ROOT`], [`TEST_LAYER`]
//! and [`TEST_CAP`] (public test data), so a case can carry real sealed frames.
//! [`rt2`] builds such a corpus with the sealing code itself.

#![forbid(unsafe_code)]

pub mod cost;
pub mod flood;
pub mod matrix;
pub mod parity;
pub mod rt2;

use lumen_arm_a::pipeline::ITEM_KINDS as ARM_A_KINDS;
use lumen_arm_b::ITEM_KINDS as ARM_B_KINDS;
use lumen_harness::{HeaderInfo, Origin, Recorder};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// Frames and bytes a producer may use per second in a full budget.
const FULL_FRAMES: usize = 2000;
const FULL_BYTES: usize = 8 << 20;

/// The version of this harness, written into every run log header. Bump it
/// when a change can alter a recorded number.
pub const HARNESS_VERSION: &str = "lumen-evidence-2";

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
    /// The kinds each arm must produce: key `b` for Arm B, `a` for Arm A.
    pub expect: BTreeMap<String, Vec<String>>,
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

fn kinds_of(id: &str, what: &str, v: &Value) -> Result<Vec<String>, String> {
    let Some(a) = v.as_array() else {
        return if v.is_null() {
            Ok(Vec::new())
        } else {
            Err(format!("{id}: {what} must be a list"))
        };
    };
    let known = |k: &str| ARM_B_KINDS.contains(&k) || ARM_A_KINDS.contains(&k);
    a.iter()
        .map(|k| {
            let k = k
                .as_str()
                .ok_or_else(|| format!("{id}: {what} entries must be strings"))?;
            if known(k) {
                Ok(k.to_owned())
            } else {
                Err(format!(
                    "{id}: unknown {what} kind {k:?}; known kinds are {ARM_B_KINDS:?} and {ARM_A_KINDS:?}"
                ))
            }
        })
        .collect()
}

fn expect_of(id: &str, v: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out = BTreeMap::new();
    if v.is_null() {
        return Ok(out);
    }
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{id}: expect must be an object with keys b and a"))?;
    for (arm, kinds) in obj {
        if arm != "b" && arm != "a" {
            return Err(format!("{id}: unknown expect key {arm:?}; use b or a"));
        }
        out.insert(arm.clone(), kinds_of(id, "expect", kinds)?);
    }
    Ok(out)
}

pub(crate) fn case_of(v: &Value) -> Result<Case, String> {
    let id = v["id"].as_str().ok_or("case without id")?.to_owned();
    let reads = v["reads"]
        .as_array()
        .ok_or_else(|| format!("{id}: no reads"))?
        .iter()
        .map(read_of)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{id}: {e}"))?;
    let forbid = kinds_of(&id, "forbid", &v["forbid"])?;
    let expect = expect_of(&id, &v["expect"])?;
    Ok(Case {
        category: v["category"].as_str().unwrap_or_default().to_owned(),
        alt_screen: v["context"]["alt_screen"].as_bool().unwrap_or(false),
        tmux_or_screen: v["context"]["tmux_or_screen"].as_bool().unwrap_or(false),
        container: v["context"]["container"].as_bool().unwrap_or(true),
        forbid,
        expect,
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
    /// Every item kind in order, with adjacent `output` and adjacent `text`
    /// items each joined.
    pub sequence: Vec<&'static str>,
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
            "commit={} features={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit,
            header.features,
            header.harness,
            header.corpus,
            header.session_matrix,
            header.sinks
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

/// The items one call produced, from either arm.
enum Items {
    A(Vec<lumen_arm_a::pipeline::Item>),
    B(Vec<lumen_arm_b::Item>),
}

impl Items {
    /// The `(kind, len)` summary of the items.
    fn summary(&self) -> Vec<(&'static str, usize)> {
        match self {
            Items::A(v) => v.iter().map(lumen_arm_a::pipeline::item_summary).collect(),
            Items::B(v) => v.iter().map(lumen_arm_b::item_summary).collect(),
        }
    }

    /// The ordinary output the layer passes on to the terminal's VT parser.
    fn output(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Items::A(v) => {
                for i in v {
                    if let lumen_arm_a::pipeline::Item::Output(b) = i {
                        out.extend_from_slice(b);
                    }
                }
            }
            Items::B(v) => {
                for i in v {
                    if let lumen_arm_b::Item::Output(b) = i {
                        out.extend_from_slice(b);
                    }
                }
            }
        }
        out
    }
}

/// One arm's layer behind the calls the driver needs.
enum AnyLayer {
    A(lumen_arm_a::pipeline::Layer),
    B(lumen_arm_b::Layer),
}

/// The layer id of the fixed test keys every replayed layer holds.
pub const TEST_LAYER: lumen_sealed::LayerId = lumen_sealed::LayerId(7);
/// The capability every test frame is sealed for.
pub const TEST_CAP: u8 = 2;
/// The root secret of the fixed test keys. It is public test data, not a secret.
pub const TEST_ROOT: [u8; 32] = [9; 32];

fn test_cap_of(_kind: u8) -> u8 {
    TEST_CAP
}

fn test_receiver() -> lumen_sealed::Receiver {
    let keys = lumen_sealed::DirectionKeys::derive(
        &lumen_sealed::LayerRoot::new(TEST_ROOT),
        TEST_LAYER,
        lumen_sealed::Direction::ToTerminal,
    );
    lumen_sealed::Receiver::new(keys, TEST_LAYER, lumen_sealed::Direction::ToTerminal)
}

impl AnyLayer {
    /// A layer that holds the fixed test keys, so a corpus can carry real sealed
    /// frames. An unsealed Arm A layer ignores them.
    fn new(arm: Arm) -> Self {
        match arm {
            Arm::B => {
                let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
                    budget: lumen_arm_b::Budget::quarter_of(FULL_FRAMES, FULL_BYTES),
                    background_as_text: false,
                });
                l.attach_sealed(test_receiver(), test_cap_of);
                AnyLayer::B(l)
            }
            Arm::ASealed | Arm::AUnsealed => {
                let state = if arm == Arm::ASealed {
                    lumen_arm_a::LayerState::Sealed
                } else {
                    lumen_arm_a::LayerState::Unsealed
                };
                let mut l = lumen_arm_a::pipeline::Layer::new(
                    lumen_arm_a::pipeline::Config {
                        budget: lumen_arm_a::pipeline::Budget::quarter_of(FULL_FRAMES, FULL_BYTES),
                    },
                    state,
                );
                l.attach_sealed(test_receiver(), test_cap_of);
                AnyLayer::A(l)
            }
        }
    }

    fn open_container(&mut self) {
        match self {
            AnyLayer::A(l) => l.open_container(),
            AnyLayer::B(l) => l.open_container(),
        }
    }

    fn feed_raw(
        &mut self,
        bytes: &[u8],
        alt_screen: bool,
        tmux_or_screen: bool,
        now_ms: u64,
    ) -> Vec<(&'static str, usize)> {
        self.feed_items(bytes, alt_screen, tmux_or_screen, now_ms)
            .summary()
    }

    fn feed_items(
        &mut self,
        bytes: &[u8],
        alt_screen: bool,
        tmux_or_screen: bool,
        now_ms: u64,
    ) -> Items {
        match self {
            AnyLayer::A(l) => Items::A(l.feed(
                bytes,
                lumen_arm_a::pipeline::Context {
                    alt_screen,
                    tmux_or_screen,
                    now_ms,
                },
            )),
            AnyLayer::B(l) => Items::B(l.feed(
                bytes,
                lumen_arm_b::Context {
                    alt_screen,
                    tmux_or_screen,
                    now_ms,
                },
            )),
        }
    }

    fn finish(&mut self) -> Vec<(&'static str, usize)> {
        self.finish_items().summary()
    }

    fn finish_items(&mut self) -> Items {
        match self {
            AnyLayer::A(l) => Items::A(l.finish()),
            AnyLayer::B(l) => Items::B(l.finish()),
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

pub(crate) fn run_case<W: Write>(
    case: &Case,
    arm: Arm,
    rec: &mut Recorder<W>,
    mut terminal: Option<&mut lumen_sinks::Terminal>,
) -> std::io::Result<CaseResult> {
    if let Some(t) = &mut terminal {
        t.new_case();
    }
    let mut layer = AnyLayer::new(arm);
    if case.container {
        layer.open_container();
    }
    rec.case(&case.id)?;
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut violations = 0;
    let mut sequence: Vec<&'static str> = Vec::new();
    let mut last_t = 0;
    let mut record =
        |t: u64, bytes_in: usize, summary: &[(&'static str, usize)], rec: &mut Recorder<W>| {
            for (kind, _) in summary {
                *kinds.entry(kind).or_insert(0) += 1;
                let joins = matches!(*kind, "output" | "text") && sequence.last() == Some(kind);
                if !joins {
                    sequence.push(kind);
                }
                if case.forbid.iter().any(|f| f == kind) {
                    violations += 1;
                }
            }
            rec.read(t, bytes_in, summary)
        };
    for r in &case.reads {
        last_t = r.t_ms;
        let items = layer.feed_items(&r.bytes, case.alt_screen, case.tmux_or_screen, r.t_ms);
        if let Some(t) = &mut terminal {
            t.feed(&items.output(), Origin::Unsealed, &case.id);
        }
        record(r.t_ms, r.bytes.len(), &items.summary(), rec)?;
    }
    let items = layer.finish_items();
    if let Some(t) = &mut terminal {
        t.feed(&items.output(), Origin::Unsealed, &case.id);
    }
    record(last_t, 0, &items.summary(), rec)?;
    Ok(CaseResult {
        id: case.id.clone(),
        kinds,
        counters: layer.counters(),
        violations,
        sequence,
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
    let mut terminal = lumen_sinks::Terminal::new();
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: arm.features().to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: corpus.revision.clone(),
        session_matrix: "none".to_owned(),
        sinks: terminal.log().table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header)?;
    let mut cases = Vec::new();
    for case in &corpus.cases {
        cases.push(run_case(case, arm, &mut rec, Some(&mut terminal))?);
    }
    rec.finish()?;
    Ok((
        Report {
            cases,
            authority: terminal.log().report(),
        },
        header,
    ))
}
