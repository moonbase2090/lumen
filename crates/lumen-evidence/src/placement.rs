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
use crate::panes::{Case, Event, PaneCorpus, PaneRect};
use lumen_detector::{DEFAULT_MAX_PAYLOAD, Detector, Event as Found};
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use lumen_panes::{Multiplexer, PaneTerminal, Rect};
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

/// The gated counts of 7.2 this mode measures.
pub const MEASURED: [&str; 4] = [
    "draw_outside_rect",
    "draw_hidden_pane",
    "stale_generation_drawn",
    "forged_container_drawn",
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
    /// Terminal-side counters, for the log.
    pub terminal: String,
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
        self.cases.iter().map(|c| c.forbidden).sum()
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
            "7.2 model level, cases={} violations={}",
            self.cases.len(),
            self.violations()
        );
        for k in MEASURED {
            let _ = writeln!(out, "{k}={}", self.total(k));
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
        for c in &self.cases {
            let _ = writeln!(
                out,
                "{} drawn={} forged_seen={} violations={} {}",
                c.id, c.drawn, c.forged_seen, c.forbidden, c.terminal
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

    /// Check every draw the terminal made since the last call.
    fn check_draws(&mut self) {
        let fresh: Vec<_> = self.term.draws()[self.seen..].to_vec();
        self.seen = self.term.draws().len();
        for d in fresh {
            self.result.drawn += 1;
            let listed = self.term.rect_of(d.pane);
            if listed.is_none() || self.detached {
                self.bump("draw_hidden_pane");
            } else if listed != Some(d.rect) || !d.rect.is_valid(self.cols, self.rows) {
                self.bump("draw_outside_rect");
            }
            if self.term.generation() != Some(d.generation) {
                self.bump("stale_generation_drawn");
            }
        }
    }

    fn send_layout(&mut self, generation: u32, panes: &[(u32, Rect)]) {
        let frame = self.mux.layout_at(generation, panes);
        let _ = self.term.receive(&frame);
        self.check_draws();
    }

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
                self.output(*pane, &read.bytes);
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
    fn output(&mut self, pane: u32, bytes: &[u8]) {
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
            let _ = self.term.receive(&frame);
        }
        self.check_draws();
    }

    fn finish(mut self) -> CaseResult {
        let c = self.term.counters();
        self.result.terminal = format!("{c:?}");
        self.result
    }
}

/// Option B for one case: the panes' bytes are one unsealed producer's output.
fn play_b(case: &Case) -> CaseResult {
    let keys = DirectionKeys::derive(&root(77), LAYER, Direction::ToTerminal);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    layer.attach_sealed(Receiver::new(keys, LAYER, Direction::ToTerminal), |_| 2);
    layer.open_container();
    let mut result = CaseResult {
        id: case.id.clone(),
        ..CaseResult::default()
    };
    let mut detached = false;
    let mut sealed_items = 0u32;
    for ev in &case.script {
        match ev {
            Event::Detach => detached = true,
            Event::Reattach => detached = false,
            Event::Output { read, .. } if !detached => {
                let items = layer.feed(
                    &read.bytes,
                    lumen_arm_b::Context {
                        now_ms: read.t_ms,
                        ..lumen_arm_b::Context::default()
                    },
                );
                sealed_items += items
                    .iter()
                    .filter(|i| lumen_arm_b::item_summary(i).0 == "sealed")
                    .count()
                    .try_into()
                    .unwrap_or(u32::MAX);
            }
            _ => {}
        }
    }
    if sealed_items > 0 {
        result
            .violations
            .insert("forged_container_drawn", sealed_items);
        if case.forbid.iter().any(|f| f == "forged_container_drawn") {
            result.forbidden += sealed_items;
        }
    }
    "no per-pane containers".clone_into(&mut result.terminal);
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
