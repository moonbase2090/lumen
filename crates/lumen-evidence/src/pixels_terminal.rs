//! Pixel mode against the real terminal binary (criteria 2.3).
//!
//! Each case is written to a file and the terminal is run headless with
//! `--snapshot`, its program a shell that turns off output translation and
//! copies the file to the terminal. The terminal's own PTY, guard, layer,
//! document and window drawing are what produce the frame; nothing here draws.
//! Every pixel in the trust-margin rectangles and the editor is compared with a
//! baseline taken from the same binary with a program that prints nothing, and
//! the baseline is taken twice.
//!
//! What this is and is not: it is the real binary's frame, drawn by the same
//! code a window presents, in a headless snapshot. It is not a capture of an
//! operating-system window on screen. It needs a Unix shell and `stty`. A case
//! that sets a context the real terminal cannot be given from outside (the
//! alternate screen flag and a multiplexer variable are harness settings, not
//! bytes) is not run and is listed as skipped. The real terminal has no open
//! command container, so Track 0 in these runs is background output.

use crate::Corpus;
use crate::pixels::{PixelCase, PixelReport, changed_in};
use lumen_term::app::{cells_for, window_for};
use lumen_term::{Frame, Renderer, layout};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Work directories handed out so far, so runs in one process never share one.
static RUNS: AtomicUsize = AtomicUsize::new(0);

/// The windows, in cells, the real terminal is sized to.
pub const WINDOWS: [(u32, u32); 2] = [(80, 24), (40, 12)];

/// What the real-terminal run measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalReport {
    /// The counts, as for the in-process mode.
    pub report: PixelReport,
    /// Cases not run, with the reason.
    pub skipped: Vec<String>,
    /// The terminal binary, as given.
    pub terminal: PathBuf,
}

fn decode(png_bytes: &[u8]) -> Result<Frame, String> {
    let mut reader = png::Decoder::new(png_bytes)
        .read_info()
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("the snapshot is not 8-bit RGBA".to_owned());
    }
    buf.truncate(info.buffer_size());
    Ok(Frame {
        width: info.width,
        height: info.height,
        rgba: buf,
    })
}

/// Run the terminal on `script` (a shell command line) and return its frame.
fn snapshot(
    terminal: &Path,
    work: &Path,
    name: &str,
    size: (u32, u32),
    scale: u32,
    script: &str,
) -> Result<Frame, String> {
    let png = work.join(format!("{name}.png"));
    let out = Command::new(terminal)
        .arg("--snapshot")
        .arg(&png)
        .arg("--size")
        .arg(format!("{}x{}", size.0, size.1))
        .arg("--scale")
        .arg(scale.to_string())
        .arg("--wait")
        .arg("5000")
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|e| format!("{}: {e}", terminal.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            terminal.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let bytes = std::fs::read(&png).map_err(|e| format!("{}: {e}", png.display()))?;
    decode(&bytes)
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

/// Run the corpus through the terminal binary at `terminal`. Frames are
/// written under `out/screenshots-terminal` when `out` is given.
///
/// # Errors
///
/// Returns a message if the terminal cannot be run, a snapshot is not the size
/// the window should have, or a file cannot be written.
pub fn run_terminal(
    corpus: &Corpus,
    scale: u32,
    terminal: &Path,
    out: Option<&Path>,
) -> Result<TerminalReport, String> {
    let renderer = Renderer::light(scale)?;
    let (cw, ch) = renderer.cell();
    let work = std::env::temp_dir().join(format!(
        "lumen-pixels-term-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let shots = out.map(|o| o.join("screenshots-terminal"));
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let keep = |name: &str, frame: &Frame| -> Result<(), String> {
        if let Some(dir) = &shots {
            let png = frame.to_png().map_err(|e| e.to_string())?;
            std::fs::write(dir.join(name), png).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let mut stable = true;
    let mut baselines = Vec::new();
    let mut cases = Vec::new();
    let mut skipped = Vec::new();
    for case in &corpus.cases {
        if case.alt_screen || case.tmux_or_screen {
            skipped.push(format!(
                "{}: a context set by the harness (alternate screen or multiplexer variable) cannot be given to the real terminal from outside",
                case.id
            ));
        }
    }
    let runnable: Vec<_> = corpus
        .cases
        .iter()
        .filter(|c| !c.alt_screen && !c.tmux_or_screen)
        .collect();
    for (cols, rows) in WINDOWS {
        let size = (cols * cw, rows * ch);
        let (c, r) = cells_for(size.0, size.1, (cw, ch));
        if (u32::try_from(c).ok(), u32::try_from(r).ok()) != (Some(cols), Some(rows)) {
            return Err(format!(
                "{size:?} pixels is {c}x{r} cells, not {cols}x{rows}"
            ));
        }
        let win = window_for(c, r);
        let l = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
        let quiet = "stty -onlcr; :";
        let first = snapshot(terminal, &work, "base-a", size, scale, quiet)?;
        let second = snapshot(terminal, &work, "base-b", size, scale, quiet)?;
        if (first.width, first.height) != size {
            return Err(format!(
                "the terminal drew {}x{} pixels, expected {}x{}",
                first.width, first.height, size.0, size.1
            ));
        }
        stable &= first == second;
        keep(&format!("baseline-{cols}x{rows}.png"), &first)?;
        for case in &runnable {
            let file = work.join(format!("{}.bin", case.id));
            let bytes: Vec<u8> = case.reads.iter().flat_map(|r| r.bytes.clone()).collect();
            std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
            let script = format!("stty -onlcr; cat '{}'", file.display());
            let frame = snapshot(terminal, &work, &case.id, size, scale, &script)?;
            keep(&format!("{}-{cols}x{rows}.png", case.id), &frame)?;
            let everything = lumen_term::Rect {
                x: 0,
                y: 0,
                w: cols,
                h: rows,
            };
            cases.push(PixelCase {
                id: case.id.clone(),
                window: (cols, rows),
                margin: changed_in(&renderer, &first, &frame, &l.margin),
                chrome: changed_in(&renderer, &first, &frame, &[l.editor]),
                anywhere: changed_in(&renderer, &first, &frame, &[everything]),
            });
        }
        baselines.push(first);
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(TerminalReport {
        report: PixelReport {
            scale,
            cell: (cw, ch),
            cases,
            baseline_stable: stable,
            baseline_digest: digest(&baselines),
        },
        skipped,
        terminal: terminal.to_path_buf(),
    })
}

/// The rectangles of the real terminal's layout, as text.
#[must_use]
pub fn terminal_rects(renderer: &Renderer) -> String {
    let (cw, ch) = renderer.cell();
    let mut out = String::new();
    for (cols, rows) in WINDOWS {
        let win = window_for(cols as usize, rows as usize);
        let l = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
        let px =
            |r: &lumen_term::Rect| format!("{},{},{},{}", r.x * cw, r.y * ch, r.w * cw, r.h * ch);
        let _ = writeln!(
            out,
            "rects {cols}x{rows} cells, {}x{} px: margin=[{}] [{}] chrome=[{}]",
            cols * cw,
            rows * ch,
            px(&l.margin[0]),
            px(&l.margin[1]),
            px(&l.editor)
        );
    }
    out
}
