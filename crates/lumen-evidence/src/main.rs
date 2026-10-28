//! `lumen-evidence --corpus DIR --commit SHA --out DIR`
//!
//! Replays the corpus through the Arm B layer. Writes `run.jsonl` and
//! `summary.txt` to the output directory and prints the summary. Exits 1 when
//! a case produced a forbidden item.

use std::path::PathBuf;
use std::process::ExitCode;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(corpus), Some(commit), Some(out)) = (
        arg(&args, "--corpus"),
        arg(&args, "--commit"),
        arg(&args, "--out"),
    ) else {
        eprintln!("usage: lumen-evidence --corpus DIR --commit SHA --out DIR");
        return ExitCode::from(2);
    };
    let out = PathBuf::from(out);
    let result = lumen_evidence::load_corpus(&PathBuf::from(corpus))
        .and_then(|c| {
            std::fs::create_dir_all(&out)
                .map(|()| c)
                .map_err(|e| e.to_string())
        })
        .and_then(|c| {
            let log = std::fs::File::create(out.join("run.jsonl")).map_err(|e| e.to_string())?;
            lumen_evidence::run(&c, &commit, std::io::BufWriter::new(log))
                .map_err(|e| e.to_string())
        });
    match result {
        Ok((report, header)) => {
            let summary = report.summary(&header);
            print!("{summary}");
            if let Err(e) = std::fs::write(out.join("summary.txt"), &summary) {
                eprintln!("summary.txt: {e}");
                return ExitCode::from(2);
            }
            if report.violations() == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
