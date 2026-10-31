//! Reader screenshot mode (criteria 2.2b): real rendered screenshots of a fixed
//! prompt set, for a panel of simulated readers, and the score of their labels.
//!
//! Each prompt is a byte stream, labelled genuine or spoofed. It is played
//! through Arm B into the terminal's document and through the plain
//! [`Passthrough`] layer (the plain-VT control), and the window is drawn by the
//! same code the terminal uses, under the render conditions of 2.3: the light
//! theme, the one bundled face, no cursor, no animation, the 80 by 24 window and
//! the scale the caller gives. The frames are written as PNGs and are never
//! edited. The harness has no readers: it writes `manifest.json`, and a later
//! call scores the labels the readers gave. Arm A has no layer in the terminal,
//! so it is not rendered here and the summary says so.

use crate::HARNESS_VERSION;
use crate::pixels::WINDOWS;
use lumen_term::{Context, Document, Layer, Passthrough, Renderer, Window};
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
pub const RENDERINGS: [&str; 2] = ["arm-b", "plain-vt"];

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
    for p in &prompts.prompts {
        let mut frames = Vec::new();
        for rendering in RENDERINGS {
            let doc = if rendering == "arm-b" {
                let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
                    budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
                    background_as_text: false,
                });
                play(&mut layer, &p.bytes, &window)
            } else {
                play(&mut Passthrough, &p.bytes, &window)
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
        if frames[0] == frames[1] {
            same_frame.push(p.id.clone());
        }
    }
    let manifest = json!({
        "corpus": prompts.revision,
        "harness": HARNESS_VERSION,
        "scale": scale,
        "window": [window.cols, window.rows],
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "render_conditions": "light theme, bundled face, no cursor, no animation",
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
        let b = self.tallies.get("arm-b")?.percent()?;
        let c = self.tallies.get("plain-vt")?.percent()?;
        Some(b > c)
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
        match self.vetoed() {
            Some(true) => {
                let _ = writeln!(out, "2.2b veto: arm-b is above the plain-VT control");
            }
            Some(false) => {
                let _ = writeln!(
                    out,
                    "2.2b veto: not triggered (a pass decides nothing about the spoofing rule)"
                );
            }
            None => {
                let _ = writeln!(out, "2.2b veto: not measured");
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
