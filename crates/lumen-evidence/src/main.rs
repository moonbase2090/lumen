//! `lumen-evidence --corpus DIR --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]`
//!
//! Replays the corpus through one arm's layer (default `b`). Writes `run.jsonl` and
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

const USAGE: &str = "usage: lumen-evidence --corpus DIR --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]\n       lumen-evidence --flood --seconds N --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]\n       lumen-evidence --matrix FILE --commit SHA --out DIR [--arm b|a]\n       lumen-evidence --cost --root DIR --base REF --commit SHA --out DIR\n       lumen-evidence --parity --channels FILE --commit SHA --out DIR";

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
    corpus: &str,
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
    let corpus = lumen_evidence::load_corpus(&PathBuf::from(corpus))?;
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
    let result = match (arg(&args, "--corpus"), arg(&args, "--matrix")) {
        (None, None) if args.iter().any(|a| a == "--flood") => match arg(&args, "--seconds") {
            Some(n) => flood(&n, &commit, &out, &arm),
            None => Err("--flood needs --seconds".to_owned()),
        },
        (None, None) if args.iter().any(|a| a == "--cost") => {
            match (arg(&args, "--root"), arg(&args, "--base")) {
                (Some(root), Some(base)) => cost(&root, &base, &commit, &out),
                _ => Err("--cost needs --root and --base".to_owned()),
            }
        }
        (None, None) if args.iter().any(|a| a == "--parity") => match arg(&args, "--channels") {
            Some(f) => parity(&f, &commit, &out),
            None => Err("--parity needs --channels".to_owned()),
        },
        (Some(corpus), None) => replay(&corpus, &commit, &out, &arm),
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
