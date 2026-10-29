//! Pixel mode (criteria 2.3): the changed pixels in the trust margin and the
//! chrome, over a corpus, against a baseline with no producer input.
//!
//! Each case is replayed through the terminal's Arm B layer into a document,
//! the window is rendered, and every pixel inside the trust-margin rectangles
//! and the editor is compared with the baseline, with no tolerance. The
//! baseline is rendered twice first: if the two differ the oracle is invalid
//! and nothing is counted.
//!
//! Render conditions: the light theme, the one bundled face, no cursor, no
//! animation, and the scale the caller gives (2.3 names 2). A baseline is per
//! platform, so the summary records the platform and a digest of the baseline.
//!
//! What is counted: the margin is the gutter and the header strip; the chrome
//! is the editor. The background strip is also terminal-drawn but it is where
//! background output is supposed to appear, so it is not counted. This renders
//! the same drawing code the window uses; it is not a screenshot of an
//! operating-system window, which needs a window that draws the layout.

use crate::Corpus;
use crate::HARNESS_VERSION;
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use lumen_term::{Context, Document, Frame, Layer, Rect, Renderer, Window, layout};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// The windows the corpus is rendered in, in cells.
pub const WINDOWS: [Window; 2] = [
    Window {
        cols: 80,
        rows: 24,
        editor_row: 19,
        editor_rows: 2,
    },
    Window {
        cols: 40,
        rows: 12,
        editor_row: 8,
        editor_rows: 2,
    },
];

/// One case in one window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelCase {
    /// Case id.
    pub id: String,
    /// The window's columns by rows.
    pub window: (u32, u32),
    /// Changed pixels in the trust margin.
    pub margin: usize,
    /// Changed pixels in the editor.
    pub chrome: usize,
    /// Changed pixels anywhere in the window: nothing drawn at all means the
    /// case did not reach the terminal, and a zero in the margin would mean nothing.
    pub anywhere: usize,
}

/// The result of a pixel run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelReport {
    /// The scale.
    pub scale: u32,
    /// The cell size in pixels.
    pub cell: (u32, u32),
    /// Every case in every window.
    pub cases: Vec<PixelCase>,
    /// The baseline taken twice matched exactly, in every window.
    pub baseline_stable: bool,
    /// A digest of the baselines, so a reader can see two runs used the same one.
    pub baseline_digest: String,
}

impl PixelReport {
    /// Changed pixels in the margin, over every case.
    #[must_use]
    pub fn margin(&self) -> usize {
        self.cases.iter().map(|c| c.margin).sum()
    }

    /// Changed pixels in the editor, over every case.
    #[must_use]
    pub fn chrome(&self) -> usize {
        self.cases.iter().map(|c| c.chrome).sum()
    }

    /// Cases whose frame differs from the baseline somewhere.
    #[must_use]
    pub fn drawn(&self) -> usize {
        self.cases.iter().filter(|c| c.anywhere > 0).count()
    }

    /// The exit status: 0 for a measured zero, 1 for a non-zero count, 3 when the
    /// baseline was not stable, there was nothing to measure, or no case drew
    /// anything (so a zero would be vacuous).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if !self.baseline_stable || self.cases.is_empty() || self.drawn() == 0 {
            3
        } else {
            u8::from(self.margin() + self.chrome() != 0)
        }
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo, rects: &str) -> String {
        self.summary_of(
            header,
            rects,
            "2.3 is a count over this drawing code, not an operating-system window",
        )
    }

    /// The summary text with `basis` stating what was measured.
    #[must_use]
    pub fn summary_of(&self, header: &HeaderInfo, rects: &str, basis: &str) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} harness={} corpus={} features={}",
            header.commit, header.harness, header.corpus, header.features
        );
        let _ = writeln!(
            out,
            "render conditions: light theme, bundled face, scale={}, cell={}x{}, no cursor, no animation, platform={}-{}",
            self.scale,
            self.cell.0,
            self.cell.1,
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let _ = writeln!(out, "baseline_digest={}", self.baseline_digest);
        if !self.baseline_stable {
            let _ = writeln!(
                out,
                "2.3=not measured (the baseline taken twice did not match, so the oracle is invalid on this build)"
            );
            return out;
        }
        if self.cases.is_empty() {
            let _ = writeln!(out, "2.3=not measured (no cases)");
            return out;
        }
        if self.drawn() == 0 {
            let _ = writeln!(
                out,
                "2.3=not measured (no case changed any pixel of the window, so the cases did not reach the drawing)"
            );
            return out;
        }
        let _ = writeln!(
            out,
            "2.3 changed_pixels margin={} chrome={} over {} case-window runs (threshold 0)",
            self.margin(),
            self.chrome(),
            self.cases.len()
        );
        let _ = writeln!(
            out,
            "{} of {} runs changed some pixel of the window outside the margin and the editor (the content and the strip)",
            self.drawn(),
            self.cases.len()
        );
        let _ = writeln!(out, "{basis}");
        out.push_str(rects);
        for c in &self.cases {
            let _ = writeln!(
                out,
                "{} {}x{} margin={} chrome={} window={}",
                c.id, c.window.0, c.window.1, c.margin, c.chrome, c.anywhere
            );
        }
        out
    }
}

/// The exact pixel rectangles of the trust margin and the chrome per window, as
/// text, for the summary and the committed list.
#[must_use]
pub fn rects_text(renderer: &Renderer) -> String {
    let (cw, ch) = renderer.cell();
    let mut out = String::new();
    for w in WINDOWS {
        let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
        let px = |r: &Rect| format!("{},{},{},{}", r.x * cw, r.y * ch, r.w * cw, r.h * ch);
        let _ = writeln!(
            out,
            "rects {}x{} cells, {}x{} px: margin=[{}] [{}] chrome=[{}]",
            w.cols,
            w.rows,
            w.cols * cw,
            w.rows * ch,
            px(&l.margin[0]),
            px(&l.margin[1]),
            px(&l.editor)
        );
    }
    out
}

fn digest(frames: &[Frame]) -> String {
    let mut h = Sha256::new();
    for f in frames {
        h.update(f.width.to_be_bytes());
        h.update(f.height.to_be_bytes());
        h.update(&f.rgba);
    }
    format!("{:x}", h.finalize())
}

/// Pixels that differ between `a` and `b` inside the union of `rects` (in cells).
#[must_use]
pub fn changed_in(renderer: &Renderer, a: &Frame, b: &Frame, rects: &[Rect]) -> usize {
    let (cw, ch) = renderer.cell();
    let mut seen = BTreeSet::new();
    for r in rects {
        for y in r.y * ch..(r.y + r.h) * ch {
            for x in r.x * cw..(r.x + r.w) * cw {
                if a.pixel(x, y) != b.pixel(x, y) {
                    seen.insert((x, y));
                }
            }
        }
    }
    seen.len()
}

fn document(case: &crate::Case, window: &Window) -> Document {
    let content_columns = usize::try_from(window.cols.saturating_sub(4))
        .unwrap_or(1)
        .max(1);
    let content_rows = usize::try_from(window.editor_row.saturating_sub(1))
        .unwrap_or(1)
        .max(1);
    let mut doc = Document::new(content_columns, content_rows);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    if case.container {
        Layer::open_container(&mut layer);
    }
    for read in &case.reads {
        let ctx = Context {
            now_ms: read.t_ms,
            alt_screen: case.alt_screen,
            tmux_or_screen: case.tmux_or_screen,
        };
        doc.receive(&mut layer, &read.bytes, ctx);
    }
    doc.finish(&mut layer);
    doc
}

/// Render the corpus at `scale`, write the run log to `log` and, if `out` is
/// given, the baseline and each case's frame as PNGs under `out/screenshots`.
///
/// # Errors
///
/// Returns a message if the face does not load or a file cannot be written.
pub fn run_pixels<W: Write>(
    corpus: &Corpus,
    commit: &str,
    scale: u32,
    out: Option<&Path>,
    log: W,
) -> Result<(PixelReport, HeaderInfo), String> {
    run_pixels_with(corpus, commit, scale, out, log, &|r, d, w| {
        r.render_window(d, w)
    })
}

/// A function that draws a window: the renderer, the document and the window.
pub type Draw<'a> = &'a dyn Fn(&Renderer, &Document, &Window) -> Frame;

/// Like [`run_pixels`], with the drawing function given. A test passes a
/// leaky one to check that the oracle sees a leak, and one whose output
/// changes between calls to check that it refuses an unstable baseline.
///
/// # Errors
///
/// Returns a message if the face does not load or a file cannot be written.
pub fn run_pixels_with<W: Write>(
    corpus: &Corpus,
    commit: &str,
    scale: u32,
    out: Option<&Path>,
    log: W,
    draw: Draw<'_>,
) -> Result<(PixelReport, HeaderInfo), String> {
    let renderer = Renderer::light(scale)?;
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: "arm-b".to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: corpus.revision.clone(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header).map_err(|e| e.to_string())?;
    let shots = out.map(|o| o.join("screenshots"));
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let save = |name: &str, frame: &Frame| -> Result<(), String> {
        if let Some(dir) = &shots {
            let png = frame.to_png().map_err(|e| e.to_string())?;
            std::fs::write(dir.join(name), png).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let mut stable = true;
    let mut baselines = Vec::new();
    let mut cases = Vec::new();
    for w in WINDOWS {
        let empty = Document::new(1, 1);
        let first = draw(&renderer, &empty, &w);
        let second = draw(&renderer, &empty, &w);
        stable &= first == second;
        save(&format!("baseline-{}x{}.png", w.cols, w.rows), &first)?;
        let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
        let margin_rects = l.margin;
        for case in &corpus.cases {
            rec.case(&format!("{}@{}x{}", case.id, w.cols, w.rows))
                .map_err(|e| e.to_string())?;
            let doc = document(case, &w);
            let frame = draw(&renderer, &doc, &w);
            save(&format!("{}-{}x{}.png", case.id, w.cols, w.rows), &frame)?;
            let margin = changed_in(&renderer, &first, &frame, &margin_rects);
            let chrome = changed_in(&renderer, &first, &frame, &[l.editor]);
            rec.read(
                0,
                0,
                &[("margin_changed", margin), ("chrome_changed", chrome)],
            )
            .map_err(|e| e.to_string())?;
            let everything = Rect {
                x: 0,
                y: 0,
                w: w.cols,
                h: w.rows,
            };
            cases.push(PixelCase {
                id: case.id.clone(),
                window: (w.cols, w.rows),
                margin,
                chrome,
                anywhere: changed_in(&renderer, &first, &frame, &[everything]),
            });
        }
        baselines.push(first);
    }
    rec.finish().map_err(|e| e.to_string())?;
    Ok((
        PixelReport {
            scale,
            cell: renderer.cell(),
            cases,
            baseline_stable: stable,
            baseline_digest: digest(&baselines),
        },
        header,
    ))
}
