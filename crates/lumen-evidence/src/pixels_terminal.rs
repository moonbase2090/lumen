//! Pixel mode against the real terminal binary (criteria 2.3).
//!
//! Each case is written to a file and the terminal is run headless with
//! `--snapshot`, its program one that copies the file to the terminal: on Unix
//! a shell that turns off output translation first, on Windows PowerShell
//! writing the file's bytes to its standard output. The terminal's own PTY, guard, layer,
//! document and window drawing are what produce the frame; nothing here draws.
//! Every pixel in the trust-margin rectangles and the editor is compared with a
//! baseline taken from the same binary with a program that prints nothing, and
//! the baseline is taken twice.
//!
//! What this is and is not: it is the real binary's frame, drawn by the same
//! code a window presents, in a headless snapshot. It is not a capture of an
//! operating-system window on screen. On Windows the terminal's pseudo-console
//! (`ConPTY`) sits between the program and the terminal, so what the terminal
//! reads is what `ConPTY` re-emits for the bytes written, not the bytes
//! themselves; the baseline is taken through the same path. A case
//! that sets a context the real terminal cannot be given from outside (the
//! alternate screen flag and a multiplexer variable are harness settings, not
//! bytes) is not run and is listed as skipped. The real terminal has no open
//! command container, so Track 0 in these runs is background output.

use crate::Corpus;
use crate::pixels::{PixelCase, PixelReport, changed_in};
use lumen_term::app::{cells_for, window_for};
use lumen_term::{Frame, Renderer, layout};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Work directories handed out so far, so runs in one process never share one.
static RUNS: AtomicUsize = AtomicUsize::new(0);

/// The longest one terminal run may take before it is killed. The terminal's
/// own `--wait` is 5 seconds; this bounds a run that never returns.
const RUN_DEADLINE: Duration = Duration::from_secs(60);

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

/// What the terminal's program does.
#[derive(Debug, Clone, Copy)]
enum Stimulus<'a> {
    /// Print nothing and exit.
    Nothing,
    /// Write the bytes of this file to the terminal, untranslated.
    File(&'a Path),
}

/// Which kind of program drives the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    /// `/bin/sh` with `stty -onlcr`.
    Unix,
    /// Windows PowerShell writing raw bytes to standard output.
    Windows,
}

impl Flavor {
    const fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// The program and arguments that give the terminal `stimulus`.
fn program_for(flavor: Flavor, stimulus: Stimulus<'_>) -> (OsString, Vec<OsString>) {
    match flavor {
        Flavor::Unix => {
            let script = match stimulus {
                Stimulus::Nothing => "stty -onlcr; :".to_owned(),
                Stimulus::File(f) => format!("stty -onlcr; cat '{}'", f.display()),
            };
            (
                OsString::from("/bin/sh"),
                vec![OsString::from("-c"), OsString::from(script)],
            )
        }
        Flavor::Windows => {
            let script = match stimulus {
                Stimulus::Nothing => "exit 0".to_owned(),
                Stimulus::File(f) => format!(
                    "$b = [System.IO.File]::ReadAllBytes('{}'); \
                     $o = [System.Console]::OpenStandardOutput(); \
                     $o.Write($b, 0, $b.Length); $o.Flush()",
                    // Inside single quotes only the quote itself needs doubling.
                    f.display().to_string().replace('\'', "''")
                ),
            };
            let root =
                std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
            let exe = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
            (
                exe.into_os_string(),
                ["-NoProfile", "-NonInteractive", "-Command"]
                    .iter()
                    .map(OsString::from)
                    .chain([OsString::from(script)])
                    .collect(),
            )
        }
    }
}

/// Run `command` with its output sent to files, and kill it after `limit`.
fn run_bounded(
    command: &mut Command,
    work: &Path,
    name: &str,
    limit: Duration,
) -> Result<(), String> {
    let out_path = work.join(format!("{name}.out"));
    let err_path = work.join(format!("{name}.err"));
    let out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let err = std::fs::File::create(&err_path).map_err(|e| e.to_string())?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("{e}"))?;
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("killed after {} seconds", limit.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    if status.success() {
        Ok(())
    } else {
        let said = std::fs::read(&err_path).unwrap_or_default();
        Err(format!(
            "exited with {status}: {}",
            String::from_utf8_lossy(&said)
        ))
    }
}

/// Run the terminal on `stimulus` and return its frame.
fn snapshot(
    terminal: &Path,
    work: &Path,
    name: &str,
    size: (u32, u32),
    scale: u32,
    stimulus: Stimulus<'_>,
) -> Result<Frame, String> {
    let png = work.join(format!("{name}.png"));
    let (program, arguments) = program_for(Flavor::host(), stimulus);
    let mut command = Command::new(terminal);
    command
        .arg("--snapshot")
        .arg(&png)
        .arg("--size")
        .arg(format!("{}x{}", size.0, size.1))
        .arg("--scale")
        .arg(scale.to_string())
        .arg("--wait")
        .arg("5000")
        .arg("--")
        .arg(program)
        .args(arguments);
    run_bounded(&mut command, work, name, RUN_DEADLINE)
        .map_err(|e| format!("{}: {e}", terminal.display()))?;
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
        let first = snapshot(terminal, &work, "base-a", size, scale, Stimulus::Nothing)?;
        let second = snapshot(terminal, &work, "base-b", size, scale, Stimulus::Nothing)?;
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
            let frame = snapshot(
                terminal,
                &work,
                &case.id,
                size,
                scale,
                Stimulus::File(&file),
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unix_program_turns_off_translation_and_copies_the_file() {
        let (program, args) = program_for(Flavor::Unix, Stimulus::File(Path::new("/tmp/a.bin")));
        assert_eq!(program, "/bin/sh");
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "stty -onlcr; cat '/tmp/a.bin'");
        let (_, quiet) = program_for(Flavor::Unix, Stimulus::Nothing);
        assert_eq!(quiet[1], "stty -onlcr; :");
    }

    #[cfg(unix)]
    #[test]
    fn a_run_that_never_returns_is_killed() {
        let dir = std::env::temp_dir().join(format!("lumen-bound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "sleep 30"]);
        let started = Instant::now();
        let r = run_bounded(&mut c, &dir, "hang", Duration::from_millis(200));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.is_err_and(|e| e.contains("killed")));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_windows_program_is_powershell_and_never_a_unix_shell() {
        let (program, args) =
            program_for(Flavor::Windows, Stimulus::File(Path::new(r"C:\t\it's.bin")));
        let program = program.to_string_lossy().to_lowercase();
        assert!(program.ends_with("powershell.exe"), "{program}");
        assert!(!program.contains("/bin/sh"));
        assert_eq!(&args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        let script = args[3].to_string_lossy();
        assert!(
            script.contains(r"ReadAllBytes('C:\t\it''s.bin')"),
            "{script}"
        );
        assert!(script.contains("OpenStandardOutput"));
        assert!(!script.contains("stty"));
        let (_, quiet) = program_for(Flavor::Windows, Stimulus::Nothing);
        assert_eq!(quiet[3], "exit 0");
    }
}
