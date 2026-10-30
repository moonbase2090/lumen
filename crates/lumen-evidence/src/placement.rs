//! Placement mode (criteria section 7): replays a pane corpus through Option A
//! or Option B and counts the outcomes of 7.2 that a model can show.
//!
//! This is model-level evidence. Option A plays the corpus through the sealed
//! relay and the outer terminal's pane handling of `lumen-panes`; Option B
//! feeds the panes' bytes to an ordinary unsealed producer's layer, which has
//! no per-pane containers. Nothing is drawn, so the pixel metrics are not
//! measured, and the model chooses fresh keys on reattach, so key reuse and
//! the replayed mark are not measured either.

use crate::HARNESS_VERSION;
use crate::panes::{Case, Classification, Event, PaneCorpus, PaneRect, ProcessEvent, Scope};
use lumen_detector::{DEFAULT_MAX_PAYLOAD, Detector, Event as Found};
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use lumen_panes::{
    Multiplexer, Outcome, PaneProcessState, PaneProcessStates, PaneTerminal, ProcessIdentity,
    Program, Rect,
};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver};
use lumen_track0::UNSEALED_MIN;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

const LAYER: LayerId = LayerId(11);

/// Which placement a run plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Option A: sealed containers from a Lumen-aware multiplexer.
    A,
    /// Option B: the multiplexer is an ordinary unsealed producer.
    B,
}

impl Placement {
    /// The name in the log header.
    #[must_use]
    pub fn features(self) -> &'static str {
        match self {
            Placement::A => "placement-a",
            Placement::B => "placement-b",
        }
    }
}

/// Why Option B cannot play a case about a pane's process.
const NOT_RUN_PROCESS: &str =
    "Option B has no per-pane process state; only the sealed Option A client reports it";

/// The gated counts of 7.2 this mode measures.
pub const MEASURED: [&str; 5] = [
    "draw_outside_rect",
    "draw_hidden_pane",
    "stale_generation_drawn",
    "forged_container_drawn",
    "replayed_frame_accepted",
];

/// Counts for one case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaseResult {
    /// Case id.
    pub id: String,
    /// Containers drawn.
    pub drawn: u32,
    /// Counts of the measured outcomes, by kind.
    pub violations: BTreeMap<&'static str, u32>,
    /// Violations of kinds the case forbids.
    pub forbidden: u32,
    /// Pane-written sealed-looking frames the multiplexer saw.
    pub forged_seen: u32,
    /// Containers the multiplexer sent with a generation other than the
    /// terminal's current one.
    pub out_of_generation_sent: u32,
    /// Of those, the ones the terminal dropped.
    pub out_of_generation_dropped: u32,
    /// Replayed frames sent.
    pub replays_sent: u32,
    /// Of those, the ones refused.
    pub replays_refused: u32,
    /// Pane-written sealed-looking frames passed through to the outer
    /// terminal's pipeline in the worst case, and how many it opened.
    pub passthrough_opened: u32,
    /// Terminal-side counters, for the log.
    pub terminal: String,
    /// Why the case was not played, if it was not.
    pub not_run: Option<&'static str>,
    /// Process-state expectations checked in this case.
    pub process_expectations: u32,
    /// Process events or expectations the model could not satisfy.
    pub process_failures: u32,
    /// Pane output reads classified at the time they arrived.
    pub classified_reads: u32,
    /// The final process records, for tests and evidence inspection.
    pub process_states: PaneProcessStates,
}

/// A whole run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The placement.
    pub placement: Placement,
    /// One result per case.
    pub cases: Vec<CaseResult>,
}

impl Report {
    /// Violations of forbidden kinds over all cases.
    #[must_use]
    pub fn violations(&self) -> u32 {
        self.cases
            .iter()
            .map(|c| c.forbidden.saturating_add(c.process_failures))
            .sum()
    }

    fn sum(&self, f: impl Fn(&CaseResult) -> u32) -> u32 {
        self.cases.iter().map(f).sum()
    }

    /// Cases that were not played.
    #[must_use]
    pub fn not_run(&self) -> usize {
        self.cases.iter().filter(|c| c.not_run.is_some()).count()
    }

    /// The total of one measured kind over all cases.
    #[must_use]
    pub fn total(&self, kind: &str) -> u32 {
        self.cases
            .iter()
            .map(|c| c.violations.get(kind).copied().unwrap_or(0))
            .sum()
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
            "7.2 model level, cases={} not_run={} violations={} of {} run",
            self.cases.len(),
            self.not_run(),
            self.violations(),
            self.cases.len() - self.not_run()
        );
        for k in MEASURED {
            let _ = writeln!(out, "{k}={}", self.total(k));
        }
        let sent = self.sum(|c| c.out_of_generation_sent);
        let dropped = self.sum(|c| c.out_of_generation_dropped);
        let _ = writeln!(
            out,
            "stale_generation_drop={dropped}/{sent} out-of-generation containers dropped (need all)"
        );
        let _ = writeln!(
            out,
            "replay_refused={}/{} replayed frames refused (need all)",
            self.sum(|c| c.replays_refused),
            self.sum(|c| c.replays_sent)
        );
        let _ = writeln!(
            out,
            "forged_passthrough_opened={} of {} pane-written sealed-looking frames, fed to the outer pipeline",
            self.sum(|c| c.passthrough_opened),
            self.sum(|c| c.forged_seen)
        );
        if self.placement == Placement::B {
            let _ = writeln!(
                out,
                "option_b_limit: panes hold no keys under B, so the replay and forged-frame counts above come from unauthenticated bytes being refused by an outer pipeline that holds keys no pane knows. They are not a replay check or a forgery check, and they say nothing about an attacker who holds genuine frames."
            );
        }
        for (k, why) in [
            (
                "key_reuse",
                "the model picks fresh keys on reattach, so reuse is not a property it can show",
            ),
            (
                "unmarked_replay",
                "the pane frames carry no replayed flag in this profile",
            ),
            ("trust_margin_pixels", "nothing is drawn"),
            ("key_custody", "the model is one process"),
            ("pane_reach", "needs the screenshot oracle"),
        ] {
            let _ = writeln!(out, "{k}=not measured ({why})");
        }
        let skipped = self.not_run();
        let process_expectations = self.sum(|c| c.process_expectations);
        if process_expectations > 0 {
            let failures = self.sum(|c| c.process_failures);
            let status = if failures == 0 { "pass" } else { "FAIL" };
            let _ = writeln!(
                out,
                "pane_process_trust={status} ({process_expectations} expectations, {} output reads classified, {failures} failures)",
                self.sum(|c| c.classified_reads)
            );
        }
        if skipped > 0 {
            let _ = writeln!(
                out,
                "pane_process_trust=not measured ({skipped} cases not run: {NOT_RUN_PROCESS})"
            );
        }
        for c in &self.cases {
            if let Some(why) = c.not_run {
                let _ = writeln!(out, "{} not run ({why})", c.id);
                continue;
            }
            let _ = writeln!(
                out,
                "{} drawn={} forged_seen={} violations={} process_expectations={} process_failures={} {}",
                c.id,
                c.drawn,
                c.forged_seen,
                c.forbidden,
                c.process_expectations,
                c.process_failures,
                c.terminal
            );
        }
        out
    }
}

fn clamp_rect(p: &PaneRect) -> Rect {
    let signed = |n: i64| i32::try_from(n).unwrap_or(if n < 0 { i32::MIN } else { i32::MAX });
    Rect {
        x: signed(p.x),
        y: signed(p.y),
        w: u32::try_from(p.w).unwrap_or(u32::MAX),
        h: u32::try_from(p.h).unwrap_or(u32::MAX),
    }
}

fn root(n: u8) -> LayerRoot {
    LayerRoot::new([n; 32])
}

/// The outer terminal's layer for the multiplexer's stream, holding keys no
/// pane knows. Anything a pane writes that opens as a sealed item here is a
/// forged frame drawn.
fn outer_pipeline() -> lumen_arm_b::Layer {
    let keys = DirectionKeys::derive(&root(77), LAYER, Direction::ToTerminal);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    layer.attach_sealed(Receiver::new(keys, LAYER, Direction::ToTerminal), |_| 2);
    layer.open_container();
    layer
}

/// Items of kind `sealed` among `items`.
fn sealed_count(items: &[lumen_arm_b::Item]) -> u32 {
    items
        .iter()
        .filter(|i| lumen_arm_b::item_summary(i).0 == "sealed")
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// Option A for one case.
struct PlayerA<'a> {
    case: &'a Case,
    cols: u32,
    rows: u32,
    attach: u8,
    mux: Multiplexer,
    term: PaneTerminal,
    detectors: BTreeMap<u32, Detector>,
    mux_gen: u32,
    mux_rects: Vec<(u32, Rect)>,
    unzoomed: Option<Vec<(u32, Rect)>>,
    detached: bool,
    seen: usize,
    last_frame: Vec<u8>,
    leak: lumen_arm_b::Layer,
    processes: PaneProcessStates,
    read_classifications: BTreeMap<u32, Vec<Classification>>,
    result: CaseResult,
}

impl<'a> PlayerA<'a> {
    fn new(case: &'a Case) -> Self {
        Self {
            case,
            cols: case.cols,
            rows: case.rows,
            attach: 1,
            mux: Multiplexer::new(&root(1), LAYER),
            term: PaneTerminal::new(&root(1), LAYER, case.cols, case.rows),
            detectors: BTreeMap::new(),
            mux_gen: 0,
            mux_rects: Vec::new(),
            unzoomed: None,
            detached: false,
            seen: 0,
            last_frame: Vec::new(),
            leak: outer_pipeline(),
            processes: PaneProcessStates::default(),
            read_classifications: BTreeMap::new(),
            result: CaseResult {
                id: case.id.clone(),
                ..CaseResult::default()
            },
        }
    }

    fn bump(&mut self, kind: &'static str) {
        *self.result.violations.entry(kind).or_insert(0) += 1;
        if self.case.forbid.iter().any(|f| f == kind) {
            self.result.forbidden += 1;
        }
    }

    fn process_failure(&mut self) {
        self.result.process_failures = self.result.process_failures.saturating_add(1);
    }

    fn identity(program: &str, start_marker: u64) -> Option<ProcessIdentity> {
        Program::new(program)
            .and_then(|program| ProcessIdentity::new(1, start_marker, program))
            .ok()
    }

    fn process_event(&mut self, pane: u32, what: &ProcessEvent) {
        match what {
            ProcessEvent::Spawn { program, listed } => {
                let Some(identity) = Self::identity(program, 1) else {
                    self.process_failure();
                    return;
                };
                self.processes
                    .insert(pane, PaneProcessState::new(identity, *listed));
            }
            ProcessEvent::Exec { program } => {
                let Some(state) = self.processes.get(pane) else {
                    self.process_failure();
                    return;
                };
                let Some(identity) = Self::identity(program, state.observed().start_marker())
                else {
                    self.process_failure();
                    return;
                };
                if let Some(state) = self.processes.get_mut(pane) {
                    state.observe_identity(identity);
                }
            }
            ProcessEvent::PidReuse => {
                let Some(state) = self.processes.get(pane) else {
                    self.process_failure();
                    return;
                };
                let start_marker = state.observed().start_marker().wrapping_add(1);
                let program = state.observed().program().clone();
                let Ok(identity) = ProcessIdentity::new(1, start_marker, program) else {
                    self.process_failure();
                    return;
                };
                if let Some(state) = self.processes.get_mut(pane) {
                    state.observe_identity(identity);
                }
            }
            ProcessEvent::JobControl { on } => {
                let Some(state) = self.processes.get_mut(pane) else {
                    self.process_failure();
                    return;
                };
                state.observe_job_control(*on);
            }
            ProcessEvent::Group { members } => {
                let Some(state) = self.processes.get_mut(pane) else {
                    self.process_failure();
                    return;
                };
                if state.observe_foreground_members(*members).is_err() {
                    self.process_failure();
                }
            }
            ProcessEvent::Foreground { program } => {
                let Ok(program) = Program::new(program) else {
                    self.process_failure();
                    return;
                };
                let Some(state) = self.processes.get_mut(pane) else {
                    self.process_failure();
                    return;
                };
                state.observe_foreground_program(Some(program));
            }
        }
    }

    fn classification(_pane: u32) -> Classification {
        // v0 classifies every pane's Track 0 as possibly remote. Process state
        // is still retained so later observations cannot erase distrust facts.
        Classification::PossiblyRemote
    }

    fn expect(&mut self, pane: u32, expected: Classification, scope: Scope) {
        self.result.process_expectations = self.result.process_expectations.saturating_add(1);
        let holds = match scope {
            Scope::Now => Self::classification(pane) == expected,
            Scope::FramesReadEarlier => self
                .read_classifications
                .get(&pane)
                .filter(|reads| !reads.is_empty())
                .is_some_and(|reads| reads.iter().all(|actual| *actual == expected)),
        };
        if !holds {
            self.process_failure();
        }
    }

    fn classify_read(&mut self, pane: u32) {
        let classification = Self::classification(pane);
        self.read_classifications
            .entry(pane)
            .or_default()
            .push(classification);
        self.result.classified_reads = self.result.classified_reads.saturating_add(1);
    }

    /// Check every draw the terminal made since the last call.
    fn check_draws(&mut self) {
        let fresh: Vec<_> = self.term.draws()[self.seen..].to_vec();
        self.seen = self.term.draws().len();
        for d in fresh {
            self.result.drawn += 1;
            let listed = self.term.rect_of(d.pane);
            if listed.is_none() || self.detached || self.term.is_desynchronised() {
                self.bump("draw_hidden_pane");
            } else if listed != Some(d.rect) || !d.rect.is_valid(self.cols, self.rows) {
                self.bump("draw_outside_rect");
            }
            if self.term.generation() != Some(d.generation) {
                self.bump("stale_generation_drawn");
            }
        }
    }

    /// Deliver one sealed frame from the multiplexer and check what it drew.
    fn deliver(&mut self, frame: Vec<u8>) -> Outcome {
        let outcome = self.term.receive(&frame);
        self.last_frame = frame;
        self.check_draws();
        outcome
    }

    fn send_layout(&mut self, generation: u32, panes: &[(u32, Rect)]) {
        let frame = self.mux.layout_at(generation, panes);
        self.deliver(frame);
    }

    #[allow(clippy::too_many_lines, reason = "one arm per script step")]
    fn step(&mut self, ev: &Event) {
        match ev {
            Event::Layout { generation, panes } => {
                if self.detached {
                    return;
                }
                let visible: Vec<(u32, Rect)> = panes
                    .iter()
                    .filter(|p| p.visible)
                    .map(|p| (p.id, clamp_rect(p)))
                    .collect();
                self.send_layout(*generation, &visible);
                if *generation > self.mux_gen {
                    self.mux_gen = *generation;
                    self.mux_rects = visible;
                }
            }
            Event::Output { pane, read } => {
                if self.detached {
                    return;
                }
                self.classify_read(*pane);
                self.output(*pane, &read.bytes, read.t_ms);
            }
            Event::Container {
                pane,
                generation,
                rect,
                kind,
                content,
            } => {
                if self.detached {
                    return;
                }
                let frame =
                    self.mux
                        .container_at(*generation, *pane, clamp_rect(rect), *kind, content);
                let out_of_generation = self.term.generation() != Some(*generation);
                let outcome = self.deliver(frame);
                if out_of_generation {
                    self.result.out_of_generation_sent += 1;
                    if outcome != Outcome::Drawn {
                        self.result.out_of_generation_dropped += 1;
                    }
                }
            }
            Event::Replay => {
                if self.detached || self.last_frame.is_empty() {
                    return;
                }
                self.result.replays_sent += 1;
                let again = self.last_frame.clone();
                let outcome = self.term.receive(&again);
                self.check_draws();
                if outcome == Outcome::Refused {
                    self.result.replays_refused += 1;
                } else {
                    self.bump("replayed_frame_accepted");
                }
            }
            Event::Foreign { kind } => {
                if self.detached {
                    return;
                }
                let frame = self.mux.raw(*kind, b"x");
                self.deliver(frame);
            }
            Event::Resize { cols, rows } => {
                self.cols = *cols;
                self.rows = *rows;
                self.term.resize(*cols, *rows);
                self.mux_rects.clear();
                self.unzoomed = None;
            }
            Event::Zoom(Some(pane)) => {
                if self.detached || self.unzoomed.is_some() {
                    return;
                }
                if self.mux_rects.iter().any(|(id, _)| id == pane) {
                    self.unzoomed = Some(self.mux_rects.clone());
                    self.mux_gen += 1;
                    let full = Rect {
                        x: 0,
                        y: 0,
                        w: self.cols,
                        h: self.rows,
                    };
                    self.mux_rects = vec![(*pane, full)];
                    let (g, r) = (self.mux_gen, self.mux_rects.clone());
                    self.send_layout(g, &r);
                }
            }
            Event::Zoom(None) => {
                if self.detached {
                    return;
                }
                if let Some(back) = self.unzoomed.take() {
                    self.mux_gen += 1;
                    self.mux_rects = back;
                    let (g, r) = (self.mux_gen, self.mux_rects.clone());
                    self.send_layout(g, &r);
                }
            }
            Event::Process { pane, what } => self.process_event(*pane, what),
            Event::Expect {
                pane,
                track0,
                scope,
            } => self.expect(*pane, *track0, *scope),
            Event::Detach => self.detached = true,
            Event::Reattach => {
                self.detached = false;
                self.attach = self.attach.wrapping_add(1);
                self.mux = Multiplexer::new(&root(self.attach), LAYER);
                self.term = PaneTerminal::new(&root(self.attach), LAYER, self.cols, self.rows);
                self.mux_gen = 0;
                self.mux_rects.clear();
                self.unzoomed = None;
                self.seen = 0;
            }
        }
    }

    /// A pane wrote bytes. The multiplexer parses its Track 0, relays each
    /// unsealed frame as a container, and treats a sealed-looking frame from
    /// a pane as pane text.
    fn output(&mut self, pane: u32, bytes: &[u8], t_ms: u64) {
        let leaked = self.leak.feed(
            bytes,
            lumen_arm_b::Context {
                now_ms: t_ms,
                ..lumen_arm_b::Context::default()
            },
        );
        let opened = sealed_count(&leaked);
        for _ in 0..opened {
            self.bump("forged_container_drawn");
        }
        self.result.passthrough_opened += opened;
        let mut found = Vec::new();
        self.detectors
            .entry(pane)
            .or_insert_with(|| Detector::new(DEFAULT_MAX_PAYLOAD))
            .feed(bytes, &mut found);
        for f in found {
            let Found::Frame { kind, payload } = f else {
                continue;
            };
            if kind < UNSEALED_MIN {
                self.result.forged_seen += 1;
                continue;
            }
            let Some((_, rect)) = self.mux_rects.iter().find(|(id, _)| *id == pane).copied() else {
                continue;
            };
            let frame = self
                .mux
                .container_at(self.mux_gen, pane, rect, kind, &payload);
            self.deliver(frame);
        }
        self.check_draws();
    }

    fn finish(mut self) -> CaseResult {
        let c = self.term.counters();
        self.result.terminal = format!("{c:?}");
        self.result.process_states = self.processes;
        self.result
    }
}

/// Option B for one case: the panes' bytes are one unsealed producer's output.
///
/// Every byte a pane writes reaches the outer terminal's pipeline, which holds
/// keys no pane knows. A sealed-looking frame from a pane is counted when a
/// per-pane detector sees it, and it is a forged frame drawn only if the
/// pipeline opens it. A replay writes the pane's last bytes again: it is
/// refused if the pipeline opens nothing the second time.
fn play_b(case: &Case) -> CaseResult {
    let mut layer = outer_pipeline();
    let mut result = CaseResult {
        id: case.id.clone(),
        ..CaseResult::default()
    };
    let mut detectors: BTreeMap<u32, Detector> = BTreeMap::new();
    let mut detached = false;
    let mut last: Option<(Vec<u8>, u64)> = None;
    let bump = |result: &mut CaseResult, kind: &'static str| {
        *result.violations.entry(kind).or_insert(0) += 1;
        if case.forbid.iter().any(|f| f == kind) {
            result.forbidden += 1;
        }
    };
    let ctx = |t_ms: u64| lumen_arm_b::Context {
        now_ms: t_ms,
        ..lumen_arm_b::Context::default()
    };
    for ev in &case.script {
        match ev {
            Event::Detach => detached = true,
            Event::Reattach => detached = false,
            Event::Output { pane, read } if !detached => {
                let opened = sealed_count(&layer.feed(&read.bytes, ctx(read.t_ms)));
                for _ in 0..opened {
                    bump(&mut result, "forged_container_drawn");
                }
                result.passthrough_opened += opened;
                let mut found = Vec::new();
                detectors
                    .entry(*pane)
                    .or_insert_with(|| Detector::new(DEFAULT_MAX_PAYLOAD))
                    .feed(&read.bytes, &mut found);
                for f in found {
                    if matches!(f, Found::Frame { kind, .. } if kind < UNSEALED_MIN) {
                        result.forged_seen += 1;
                    }
                }
                last = Some((read.bytes.clone(), read.t_ms));
            }
            Event::Replay if !detached => {
                if let Some((bytes, t_ms)) = &last {
                    result.replays_sent += 1;
                    if sealed_count(&layer.feed(bytes, ctx(*t_ms))) == 0 {
                        result.replays_refused += 1;
                    } else {
                        bump(&mut result, "replayed_frame_accepted");
                    }
                }
            }
            _ => {}
        }
    }
    result.terminal = format!("no per-pane containers; {:?}", layer.counters());
    result
}

/// Replay `corpus` through `placement`, writing the run log to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run_placement<W: Write>(
    corpus: &PaneCorpus,
    commit: &str,
    placement: Placement,
    log: W,
) -> std::io::Result<(Report, HeaderInfo)> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: placement.features().to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: corpus.revision.clone(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header)?;
    let mut cases = Vec::new();
    for case in &corpus.cases {
        rec.case(&case.id)?;
        if placement == Placement::B && case.script.iter().any(Event::is_process_trust) {
            cases.push(CaseResult {
                id: case.id.clone(),
                not_run: Some(NOT_RUN_PROCESS),
                ..CaseResult::default()
            });
            continue;
        }
        let result = match placement {
            Placement::A => {
                let mut p = PlayerA::new(case);
                for ev in &case.script {
                    p.step(ev);
                    if let Event::Output { read, .. } = ev {
                        rec.read(
                            read.t_ms,
                            read.bytes.len(),
                            &[("container", p.result.drawn as usize)],
                        )?;
                    }
                }
                p.finish()
            }
            Placement::B => {
                for ev in &case.script {
                    if let Event::Output { read, .. } = ev {
                        rec.read(read.t_ms, read.bytes.len(), &[])?;
                    }
                }
                play_b(case)
            }
        };
        cases.push(result);
    }
    rec.finish()?;
    Ok((Report { placement, cases }, header))
}
