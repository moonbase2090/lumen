//! Reader screenshot mode (criteria 2.2b): real rendered screenshots of a fixed
//! prompt set, for a panel of simulated readers, and the score of their labels.
//!
//! Each prompt is a byte stream, labelled genuine or spoofed. It is played
//! through Arm A and Arm B into the terminal's document and through the plain
//! [`Passthrough`] layer (the plain-VT control), and the window is drawn by the
//! same code the terminal uses, under the render conditions of 2.3: the light
//! theme, the one bundled face, no cursor, no animation, the 80 by 24 window and
//! the scale the caller gives. The frames are written as PNGs and are never
//! edited. The harness has no readers: it writes `manifest.json`, and a later
//! call scores the labels the readers gave.
//!
//! Arm A has no layer in the terminal, so this mode wraps Arm A's pipeline in
//! [`ArmA`], which hands its items to the document unchanged. The layer is
//! unsealed, like the Arm B layer here: neither holds keys, so Arm A shows
//! Track 0 as escaped text, its rule for an unsealed layer.

use crate::HARNESS_VERSION;
use crate::pixels::WINDOWS;
use lumen_arm_a::LayerState;
use lumen_arm_a::pipeline as arm_a;
use lumen_term::{Context, Document, Layer, Notice, Passthrough, Piece, Renderer, Window};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// What a prompt really is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// The shell's own prompt.
    Genuine,
    /// Output that tries to pass for the terminal's own display.
    Spoofed,
}

impl Label {
    fn name(self) -> &'static str {
        match self {
            Label::Genuine => "genuine",
            Label::Spoofed => "spoofed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "genuine" => Some(Label::Genuine),
            "spoofed" => Some(Label::Spoofed),
            _ => None,
        }
    }
}

/// One prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Prompt id.
    pub id: String,
    /// What it is.
    pub label: Label,
    /// The bytes the producer writes.
    pub bytes: Vec<u8>,
}

/// The prompts of a corpus revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompts {
    /// The corpus revision.
    pub revision: String,
    /// The prompts, in file order.
    pub prompts: Vec<Prompt>,
}

/// The renderings a prompt gets.
pub const RENDERINGS: [&str; 3] = ["arm-a", "arm-b", "plain-vt"];

/// The arms the panel's labels can veto, each against the plain-VT control.
pub const ARMS: [&str; 2] = ["arm-a", "arm-b"];

/// Arm A's pipeline as a terminal layer. Each item becomes the piece of the
/// same meaning; nothing is added, dropped or reordered.
#[derive(Debug)]
pub struct ArmA(pub arm_a::Layer);

impl ArmA {
    /// An unsealed Arm A layer with the unauthenticated producer's budget.
    #[must_use]
    pub fn unsealed() -> Self {
        Self(arm_a::Layer::new(
            arm_a::Config {
                budget: arm_a::Budget::quarter_of(2000, 8 << 20),
            },
            LayerState::Unsealed,
        ))
    }

    fn pieces(items: Vec<arm_a::Item>) -> Vec<Piece> {
        items
            .into_iter()
            .map(|item| match item {
                arm_a::Item::Output(bytes) => Piece::Bytes(bytes),
                arm_a::Item::Block(block) => Piece::Rich(block),
                arm_a::Item::Text(text) => Piece::Text(text),
                arm_a::Item::Degraded => Piece::Notice(Notice::Degraded),
                arm_a::Item::Pinned => Piece::Notice(Notice::Pinned),
                arm_a::Item::Sealed { header, body } => Piece::Sealed {
                    kind: header.kind,
                    body,
                },
            })
            .collect()
    }
}

impl Layer for ArmA {
    fn feed(&mut self, bytes: &[u8], ctx: Context) -> Vec<Piece> {
        Self::pieces(self.0.feed(
            bytes,
            arm_a::Context {
                alt_screen: ctx.alt_screen,
                tmux_or_screen: ctx.tmux_or_screen,
                now_ms: ctx.now_ms,
            },
        ))
    }

    fn finish(&mut self) -> Vec<Piece> {
        Self::pieces(self.0.finish())
    }

    fn open_container(&mut self) {
        self.0.open_container();
    }

    fn close_container(&mut self) {
        self.0.close_container();
    }

    fn sealed_abort(&mut self, now_ms: u64) -> Vec<Piece> {
        Self::pieces(self.0.sealed_abort(now_ms))
    }
}

/// Load `readers-*.json` from `dir`.
///
/// # Errors
///
/// Returns a message for a missing or malformed file, a repeated id, an unknown
/// label or odd hex, or if there are no prompts or no spoofed or no genuine prompt.
pub fn load_prompts(dir: &Path) -> Result<Prompts, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut revision = None;
    let mut prompts: Vec<Prompt> = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let doc: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        let rev = doc["revision"]
            .as_str()
            .ok_or_else(|| format!("{}: no revision", f.display()))?;
        if revision.as_deref().is_some_and(|r| r != rev) {
            return Err(format!("{}: revision {rev} differs", f.display()));
        }
        revision = Some(rev.to_owned());
        for p in doc["prompts"].as_array().map_or(&[][..], Vec::as_slice) {
            let id = p["id"].as_str().ok_or("a prompt has no id")?.to_owned();
            let label = p["label"]
                .as_str()
                .and_then(Label::parse)
                .ok_or_else(|| format!("{id}: label must be genuine or spoofed"))?;
            let bytes = crate::unhex(p["hex"].as_str().ok_or_else(|| format!("{id}: no hex"))?)
                .map_err(|e| format!("{id}: {e}"))?;
            if prompts.iter().any(|x| x.id == id) {
                return Err(format!("duplicate prompt id {id}"));
            }
            prompts.push(Prompt { id, label, bytes });
        }
    }
    let revision = revision.ok_or("no prompt files")?;
    for want in [Label::Genuine, Label::Spoofed] {
        if !prompts.iter().any(|p| p.label == want) {
            return Err(format!("the prompt set has no {} prompt", want.name()));
        }
    }
    Ok(Prompts { revision, prompts })
}

fn play(layer: &mut dyn Layer, bytes: &[u8], window: &Window) -> Document {
    let columns = usize::try_from(window.cols.saturating_sub(4))
        .unwrap_or(1)
        .max(1);
    let rows = usize::try_from(window.editor_row.saturating_sub(1))
        .unwrap_or(1)
        .max(1);
    let mut doc = Document::new(columns, rows);
    layer.open_container();
    doc.receive(layer, bytes, Context::default());
    doc.finish(layer);
    doc
}

/// The screenshots a run wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shots {
    /// The manifest, as written to `manifest.json`.
    pub manifest: Value,
    /// Prompts whose Arm B frame equals the plain-VT frame. A reader cannot tell
    /// those apart by the screenshot, so a spoof there is not one Arm B shows.
    pub same_frame: Vec<String>,
    /// Prompts whose Arm A frame equals the plain-VT frame, likewise for Arm A.
    pub same_frame_arm_a: Vec<String>,
}

/// Render every prompt in every rendering and write the PNGs and `manifest.json`
/// under `out/readers`.
///
/// # Errors
///
/// Returns a message if the face does not load or a file cannot be written.
pub fn render_prompts(prompts: &Prompts, scale: u32, out: &Path) -> Result<Shots, String> {
    let renderer = Renderer::light(scale)?;
    let window = WINDOWS[0];
    let dir = out.join("readers");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut entries = Vec::new();
    let mut same_frame = Vec::new();
    let mut same_frame_arm_a = Vec::new();
    for p in &prompts.prompts {
        let mut frames = Vec::new();
        for rendering in RENDERINGS {
            let doc = match rendering {
                "arm-a" => play(&mut ArmA::unsealed(), &p.bytes, &window),
                "arm-b" => {
                    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
                        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
                        background_as_text: false,
                    });
                    play(&mut layer, &p.bytes, &window)
                }
                _ => play(&mut Passthrough, &p.bytes, &window),
            };
            let frame = renderer.render_window(&doc, &window);
            let png = frame.to_png().map_err(|e| e.to_string())?;
            let file = format!("{}-{rendering}.png", p.id);
            std::fs::write(dir.join(&file), &png).map_err(|e| e.to_string())?;
            entries.push(json!({
                "prompt": p.id,
                "rendering": rendering,
                "file": file,
                "sha256": format!("{:x}", Sha256::digest(&png)),
                "label": p.label.name(),
            }));
            frames.push(frame);
        }
        // In the order of RENDERINGS: arm-a, arm-b, plain-vt.
        if frames[1] == frames[2] {
            same_frame.push(p.id.clone());
        }
        if frames[0] == frames[2] {
            same_frame_arm_a.push(p.id.clone());
        }
    }
    let manifest = json!({
        "corpus": prompts.revision,
        "harness": HARNESS_VERSION,
        "scale": scale,
        "window": [window.cols, window.rows],
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "render_conditions": "light theme, bundled face, no cursor, no animation",
        "layers": "arm-a and arm-b unsealed (no keys); plain-vt is the pass-through layer",
        "shots": entries,
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(Shots {
        manifest,
        same_frame,
        same_frame_arm_a,
    })
}

/// What the panel did with one rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// Spoofed prompts shown to a reader.
    pub spoofed_shown: usize,
    /// Of those, the ones the reader called genuine.
    pub accepted: usize,
}

impl Tally {
    /// Percent of spoofed prompts accepted as genuine, or `None` if none were shown.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // counts of shots, far below 2^52
    pub fn percent(&self) -> Option<f64> {
        (self.spoofed_shown > 0).then(|| self.accepted as f64 * 100.0 / self.spoofed_shown as f64)
    }
}

/// The panel's score.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    /// Readers in the panel.
    pub readers: usize,
    /// Per rendering.
    pub tallies: BTreeMap<String, Tally>,
}

impl Score {
    /// Arm B is vetoed when its accepted-spoof fraction is above the control's.
    #[must_use]
    pub fn vetoed(&self) -> Option<bool> {
        self.arm_vetoed("arm-b")
    }

    /// `arm` (`arm-a` or `arm-b`) is vetoed when its accepted-spoof fraction is
    /// above the control's on the same prompts.
    #[must_use]
    pub fn arm_vetoed(&self, arm: &str) -> Option<bool> {
        let a = self.tallies.get(arm)?.percent()?;
        let c = self.tallies.get("plain-vt")?.percent()?;
        Some(a > c)
    }

    /// Whether any arm is vetoed.
    #[must_use]
    pub fn any_vetoed(&self) -> bool {
        ARMS.iter().any(|arm| self.arm_vetoed(arm) == Some(true))
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "2.2b readers={}", self.readers);
        for (name, t) in &self.tallies {
            match t.percent() {
                Some(p) => {
                    let _ = writeln!(
                        out,
                        "2.2b {name} accepted_spoofs={}/{} ({p:.1}%)",
                        t.accepted, t.spoofed_shown
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "2.2b {name} not measured (no spoofed prompt was labelled)"
                    );
                }
            }
        }
        for arm in ARMS {
            match self.arm_vetoed(arm) {
                Some(true) => {
                    let _ = writeln!(out, "2.2b veto: {arm} is above the plain-VT control");
                }
                Some(false) => {
                    let _ = writeln!(
                        out,
                        "2.2b veto: {arm} not triggered (a pass decides nothing about the spoofing rule)"
                    );
                }
                None => {
                    let _ = writeln!(out, "2.2b veto: {arm} not measured");
                }
            }
        }
        out
    }
}

/// Score the readers' labels against the manifest. `labels` is
/// `{"reader-1": {"<prompt>/<rendering>": "genuine"|"spoofed", ...}, ...}`.
/// Every reader must label every shot, and only shots in the manifest.
///
/// # Errors
///
/// Returns a message for a missing or unknown key, a label that is neither
/// genuine nor spoofed, or a panel with no reader.
pub fn score(manifest: &Value, labels: &Value) -> Result<Score, String> {
    let shots = manifest["shots"]
        .as_array()
        .ok_or("manifest has no shots")?;
    let readers = labels
        .as_object()
        .ok_or("labels must be an object of readers")?;
    if readers.is_empty() {
        return Err("the panel has no reader".to_owned());
    }
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for (reader, given) in readers {
        let given = given
            .as_object()
            .ok_or_else(|| format!("{reader}: labels must be an object"))?;
        for key in given.keys() {
            let known = shots.iter().any(|s| {
                format!(
                    "{}/{}",
                    s["prompt"].as_str().unwrap_or(""),
                    s["rendering"].as_str().unwrap_or("")
                ) == *key
            });
            if !known {
                return Err(format!("{reader}: {key} is not a shot in the manifest"));
            }
        }
        for s in shots {
            let prompt = s["prompt"].as_str().unwrap_or("");
            let rendering = s["rendering"].as_str().unwrap_or("");
            let key = format!("{prompt}/{rendering}");
            let said = given
                .get(&key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{reader}: no label for {key}"))?;
            let said = Label::parse(said)
                .ok_or_else(|| format!("{reader}: {key}: label must be genuine or spoofed"))?;
            let t = tallies.entry(rendering.to_owned()).or_default();
            if s["label"].as_str() == Some("spoofed") {
                t.spoofed_shown += 1;
                if said == Label::Genuine {
                    t.accepted += 1;
                }
            }
        }
    }
    Ok(Score {
        readers: readers.len(),
        tallies,
    })
}
