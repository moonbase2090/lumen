//! `lumen-evidence --corpus DIR [--corpus DIR ...] --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]`
//!
//! Replays the corpus (several directories run as one, so one verdict covers
//! them all) through one arm's layer (default `b`). Writes `run.jsonl` and
//! `summary.txt` to the output directory and prints the summary. Exits 1 when
//! a case produced a forbidden item.
//!
//! `lumen-evidence --flood --seconds N --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]`
//!
//! Flood mode (criteria 2.6): an unauthenticated producer floods one command
//! for N seconds. Exits 1 when the budget or memory limit was exceeded.
//!
//! `lumen-evidence --cost --root DIR --base REF --commit SHA --out DIR`
//!
//! Cost mode (criteria 2.8): counts what each arm crate adds to the shared
//! baseline in the source tree at DIR. Reported only; exits 0 when it ran.
//!
//! `lumen-evidence --parity --channels FILE --commit SHA --out DIR`
//!
//! Parity mode (criteria 2.9): counts the declared channels that accept more
//! than a PTY write could. Exits 0 for a measured zero, 1 for a non-zero
//! count and 3 when no channel is declared (not measured).
//!
//! `lumen-evidence --decoder [--ledger FILE] --commit SHA --out DIR`
//!
//! Decoder robustness mode (criteria 2.5): judges a decoder fuzzing ledger
//! against the threshold. Exits 0 when the gate holds, 1 when it does not and
//! 3 when nothing was measured (no ledger, or a ledger with no format).
//!
//! `lumen-evidence --render-log [--log FILE] --commit SHA --out DIR`
//!
//! Render-log mode (criteria 2.6): judges the UI stall, frame rate and
//! lying-decoder memory a terminal logged. Exits 0 when every part is within
//! its limit, 1 when a measured part is over, and 3 when a part is not measured.
//!
//! `lumen-evidence --panes DIR --placement a|b --commit SHA --out DIR`
//!
//! Placement mode (criteria 7.2): replays the pane corpus through Option A or
//! Option B at the model level. Exits 1 when a case produced a forbidden
//! outcome.
//!
//! `lumen-evidence --screenshots --corpus DIR [--corpus DIR ...] [--scale N] [--terminal PATH] --commit SHA --out DIR`
//!
//! Pixel mode (criteria 2.3): changed pixels in the trust margin and the editor
//! against a baseline, over the corpus. Exits 0 for a measured zero, 1 for a
//! non-zero count and 3 when the baseline was not stable or nothing was measured.
//!
//! `lumen-evidence --matrix FILE --commit SHA --out DIR [--arm b|a]`
//!
//! Reach mode (criteria 2.1): replays the session matrix and reports how many
//! sessions show what the arm specifies. Exits 0 when the run completes,
//! whatever the count; the count is the result.

use std::path::PathBuf;
use std::process::ExitCode;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

const USAGE: &str = "usage: lumen-evidence --corpus DIR --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]\n       lumen-evidence --flood --seconds N --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]\n       lumen-evidence --matrix FILE --commit SHA --out DIR [--arm b|a]\n       lumen-evidence --cost --root DIR --base REF --commit SHA --out DIR\n       lumen-evidence --parity --channels FILE --commit SHA --out DIR\n       lumen-evidence --screenshots --corpus DIR [--corpus DIR ...] [--scale N] [--terminal PATH] --commit SHA --out DIR\n       lumen-evidence --decoder [--ledger FILE] --commit SHA --out DIR\n       lumen-evidence --render-log [--log FILE] --commit SHA --out DIR\n       lumen-evidence --panes DIR --placement a|b --commit SHA --out DIR";

/// Write the log and summary for a finished run and print the summary.
fn finish(out: &std::path::Path, summary: &str) -> Result<(), String> {
    print!("{summary}");
    std::fs::write(out.join("summary.txt"), summary).map_err(|e| format!("summary.txt: {e}"))
}

fn open_log(out: &std::path::Path) -> Result<std::io::BufWriter<std::fs::File>, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::File::create(out.join("run.jsonl"))
        .map(std::io::BufWriter::new)
        .map_err(|e| e.to_string())
}

fn replay(
    corpus: &[String],
    commit: &str,
    out: &std::path::Path,
    arm: &str,
) -> Result<ExitCode, String> {
    let arm = match arm {
        "b" => lumen_evidence::Arm::B,
        "a-sealed" => lumen_evidence::Arm::ASealed,
        "a-unsealed" => lumen_evidence::Arm::AUnsealed,
        other => {
            return Err(format!(
                "unknown --arm {other:?}; use b, a-sealed or a-unsealed"
            ));
        }
    };
    let dirs: Vec<PathBuf> = corpus.iter().map(PathBuf::from).collect();
    let dirs: Vec<&std::path::Path> = dirs.iter().map(PathBuf::as_path).collect();
    let corpus = lumen_evidence::load_corpora(&dirs)?;
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::run_arm(&corpus, commit, arm, log).map_err(|e| e.to_string())?;
    finish(out, &report.summary(&header))?;
    Ok(if report.violations() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cost(root: &str, base: &str, commit: &str, out: &std::path::Path) -> Result<ExitCode, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let log = std::fs::File::create(out.join("run.jsonl")).map_err(|e| e.to_string())?;
    let (report, header) = lumen_evidence::cost::run_cost(
        &PathBuf::from(root),
        base,
        commit,
        std::io::BufWriter::new(log),
    )?;
    finish(out, &report.summary(&header))?;
    Ok(ExitCode::SUCCESS)
}

fn flood(
    seconds: &str,
    commit: &str,
    out: &std::path::Path,
    arm: &str,
) -> Result<ExitCode, String> {
    let arm = parse_arm(arm)?;
    let seconds: u64 = seconds
        .parse()
        .map_err(|_| format!("--seconds {seconds:?} is not a whole number"))?;
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::flood::run_flood(arm, commit, std::time::Duration::from_secs(seconds), log)
            .map_err(|e| e.to_string())?;
    finish(out, &report.summary(&header))?;
    std::fs::write(
        out.join("render.json"),
        report.render_log(&format!("{} flood {commit}", header.features)),
    )
    .map_err(|e| format!("render.json: {e}"))?;
    Ok(
        if report.within_budget() && report.memory_ok() != Some(false) {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn parse_arm(arm: &str) -> Result<lumen_evidence::Arm, String> {
    match arm {
        "b" => Ok(lumen_evidence::Arm::B),
        "a-sealed" => Ok(lumen_evidence::Arm::ASealed),
        "a-unsealed" => Ok(lumen_evidence::Arm::AUnsealed),
        other => Err(format!(
            "unknown --arm {other:?}; use b, a-sealed or a-unsealed"
        )),
    }
}

fn parity(channels: &str, commit: &str, out: &std::path::Path) -> Result<ExitCode, String> {
    let catalogue = lumen_evidence::parity::load_catalogue(&PathBuf::from(channels))?;
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::parity::run_parity(&catalogue, commit, log).map_err(|e| e.to_string())?;
    finish(out, &report.summary(&header))?;
    Ok(ExitCode::from(report.exit_code()))
}

fn decoder(ledger: Option<&str>, commit: &str, out: &std::path::Path) -> Result<ExitCode, String> {
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::decoder::run_decoder(ledger.map(std::path::Path::new), commit, log)?;
    finish(out, &report.summary(&header))?;
    Ok(match report {
        lumen_evidence::decoder::Report::NotMeasured(_) => ExitCode::from(3),
        _ if report.passes() => ExitCode::SUCCESS,
        lumen_evidence::decoder::Report::Judged { .. } => ExitCode::from(1),
    })
}

fn render_log(log: Option<&str>, commit: &str, out: &std::path::Path) -> Result<ExitCode, String> {
    let log_out = open_log(out)?;
    let (report, header) =
        lumen_evidence::render::run_render(log.map(std::path::Path::new), commit, log_out)?;
    finish(out, &report.summary(&header))?;
    Ok(ExitCode::from(report.exit_code()))
}

fn placement(
    dir: &str,
    which: &str,
    commit: &str,
    out: &std::path::Path,
) -> Result<ExitCode, String> {
    let which = match which {
        "a" => lumen_evidence::placement::Placement::A,
        "b" => lumen_evidence::placement::Placement::B,
        other => return Err(format!("unknown --placement {other:?}; use a or b")),
    };
    let corpus = lumen_evidence::panes::load_panes(&PathBuf::from(dir))?;
    let log = open_log(out)?;
    let (report, header) = lumen_evidence::placement::run_placement(&corpus, commit, which, log)
        .map_err(|e| e.to_string())?;
    finish(out, &report.summary(&header))?;
    Ok(if report.violations() > 0 {
        ExitCode::from(1)
    } else if !report.cases.is_empty() && report.not_run() == report.cases.len() {
        // Nothing was played, so nothing was measured.
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}

fn screenshots_terminal(
    corpora: &[String],
    commit: &str,
    out: &std::path::Path,
    scale: &str,
    terminal: &str,
) -> Result<ExitCode, String> {
    let scale: u32 = scale
        .parse()
        .ok()
        .filter(|s| (1..=4).contains(s))
        .ok_or_else(|| format!("--scale {scale:?} must be 1 to 4 (criteria 2.3 names 2)"))?;
    let dirs: Vec<PathBuf> = corpora.iter().map(PathBuf::from).collect();
    let dirs: Vec<&std::path::Path> = dirs.iter().map(PathBuf::as_path).collect();
    let corpus = lumen_evidence::load_corpora(&dirs)?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let run = lumen_evidence::pixels_terminal::run_terminal(
        &corpus,
        scale,
        std::path::Path::new(terminal),
        Some(out),
    )?;
    let header = lumen_harness::HeaderInfo {
        commit: commit.to_owned(),
        features: "terminal-binary".to_owned(),
        harness: lumen_evidence::HARNESS_VERSION.to_owned(),
        corpus: corpus.revision.clone(),
        session_matrix: "none".to_owned(),
        sinks: "none".to_owned(),
    };
    let rects = lumen_evidence::Renderer::light(scale)
        .map(|r| lumen_evidence::pixels_terminal::terminal_rects(&r))
        .unwrap_or_default();
    let mut text = run.report.summary_of(
        &header,
        &rects,
        "2.3 is a count over the frame the real terminal binary draws headless (--snapshot); it is not a capture of an operating-system window on screen",
    );
    for s in &run.skipped {
        text.push_str(&format!("skipped {s}\n"));
    }
    finish(out, &text)?;
    Ok(ExitCode::from(run.report.exit_code()))
}

fn screenshots(
    corpora: &[String],
    commit: &str,
    out: &std::path::Path,
    scale: &str,
) -> Result<ExitCode, String> {
    let scale: u32 = scale
        .parse()
        .ok()
        .filter(|s| (1..=4).contains(s))
        .ok_or_else(|| format!("--scale {scale:?} must be 1 to 4 (criteria 2.3 names 2)"))?;
    let dirs: Vec<PathBuf> = corpora.iter().map(PathBuf::from).collect();
    let dirs: Vec<&std::path::Path> = dirs.iter().map(PathBuf::as_path).collect();
    let corpus = lumen_evidence::load_corpora(&dirs)?;
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::pixels::run_pixels(&corpus, commit, scale, Some(out), log)?;
    let rects = lumen_evidence::Renderer::light(scale)
        .map(|r| lumen_evidence::pixels::rects_text(&r))
        .unwrap_or_default();
    finish(out, &report.summary(&header, &rects))?;
    Ok(ExitCode::from(report.exit_code()))
}

fn reach(matrix: &str, commit: &str, out: &std::path::Path, arm: &str) -> Result<ExitCode, String> {
    let arm = match arm {
        "b" => lumen_evidence::matrix::ReachArm::B,
        "a" => lumen_evidence::matrix::ReachArm::A,
        other => return Err(format!("unknown --arm {other:?} for --matrix; use b or a")),
    };
    let matrix = lumen_evidence::matrix::load_matrix(&PathBuf::from(matrix))?;
    let log = open_log(out)?;
    let (report, header) =
        lumen_evidence::matrix::run_reach(&matrix, commit, arm, log).map_err(|e| e.to_string())?;
    finish(out, &report.summary(&header))?;
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(commit), Some(out)) = (arg(&args, "--commit"), arg(&args, "--out")) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let out = PathBuf::from(out);
    let arm = arg(&args, "--arm").unwrap_or_else(|| "b".to_owned());
    let corpora: Vec<String> = args
        .windows(2)
        .filter(|w| w[0] == "--corpus")
        .map(|w| w[1].clone())
        .collect();
    let result = match (corpora.first().map(|_| ()), arg(&args, "--matrix")) {
        (None, None) if args.iter().any(|a| a == "--flood") => match arg(&args, "--seconds") {
            Some(n) => flood(&n, &commit, &out, &arm),
            None => Err("--flood needs --seconds".to_owned()),
        },
        (None, None) if args.iter().any(|a| a == "--decoder") => {
            decoder(arg(&args, "--ledger").as_deref(), &commit, &out)
        }
        (None, None) if args.iter().any(|a| a == "--cost") => {
            match (arg(&args, "--root"), arg(&args, "--base")) {
                (Some(root), Some(base)) => cost(&root, &base, &commit, &out),
                _ => Err("--cost needs --root and --base".to_owned()),
            }
        }
        (None, None) if args.iter().any(|a| a == "--render-log") => {
            render_log(arg(&args, "--log").as_deref(), &commit, &out)
        }
        (None, None) if args.iter().any(|a| a == "--panes") => {
            match (arg(&args, "--panes"), arg(&args, "--placement")) {
                (Some(dir), Some(which)) => placement(&dir, &which, &commit, &out),
                _ => Err("--panes needs a directory and --placement a|b".to_owned()),
            }
        }
        (None, None) if args.iter().any(|a| a == "--parity") => match arg(&args, "--channels") {
            Some(f) => parity(&f, &commit, &out),
            None => Err("--parity needs --channels".to_owned()),
        },
        (Some(()), None) if args.iter().any(|a| a == "--screenshots") => {
            let scale = arg(&args, "--scale").unwrap_or_else(|| "2".to_owned());
            match arg(&args, "--terminal") {
                Some(t) => screenshots_terminal(&corpora, &commit, &out, &scale, &t),
                None => screenshots(&corpora, &commit, &out, &scale),
            }
        }
        (Some(()), None) => replay(&corpora, &commit, &out, &arm),
        (None, Some(matrix)) => reach(&matrix, &commit, &out, &arm),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    result.unwrap_or_else(|e| {
        eprintln!("{e}");
        ExitCode::from(2)
    })
}
