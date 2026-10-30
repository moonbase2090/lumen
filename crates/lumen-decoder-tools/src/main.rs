//! `decoder-fuzz-tools`: command line over the library.
//!
//! - `classify LOG` prints the crash kind of a libFuzzer log and exits 1 if it
//!   reports a crash, 0 if not.
//! - `replay-kind RECORD` prints `memory` or `asan-artifact` for a replay record
//!   of an `oom-*` artifact (see `scripts/decoder-replay.py`).
//! - `identity ROOT [RUSTFLAGS] [decoder|image-path]` prints a build identity.
//! - `format NAME SAMPLES CRASHES` prints one format's ledger entry.
//! - `escape TESTLOG REVISION` prints `{revision, passed, total}` for the G4
//!   suite from a `cargo test` log.
//! - `miri-check RECORD [COMMITTED]` validates a Miri record, or compares two.
//! - `gate-status SAMPLES CRASHES [MIN WINDOW]` prints where a format stands against the gate.
//! - `slice-seconds SLICE WORKERS CAP DONE RATE|- WALL_LEFT|-` prints the next slice length.
//! - `learn-rate PREVIOUS|- CPU WORKERS WALL` prints the rate to keep.
//! - `annotate LEDGER NAME=FILE...` prints the ledger with a `notes` object.
//! - `ledger REVISION DECODER_BUILD IMAGE_PATH_BUILD ESCAPE_JSON ENTRY_JSON...`
//!   prints the ledger.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use lumen_decoder_tools::{classify, escape, gate, identity, ledger, miri, slice};
use serde_json::{Value, json};

#[allow(clippy::too_many_lines, reason = "one arm per subcommand")]
fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or("missing argument");
    match arg(0)? {
        "classify" => match classify::classify(&fs::read_to_string(arg(1)?)?) {
            Some(k) => {
                println!("{}", k.word());
                Ok(ExitCode::from(1))
            }
            None => Ok(ExitCode::SUCCESS),
        },
        "replay-kind" => {
            let record: Value = serde_json::from_str(&fs::read_to_string(arg(1)?)?)?;
            let kind =
                classify::classify_replay(&record).ok_or("the replay record has no peak_rss_kb")?;
            println!("{}", kind.word());
            Ok(ExitCode::SUCCESS)
        }
        "identity" => {
            let root = Path::new(arg(1)?);
            let flags = args.get(2).map_or("", String::as_str);
            let rustc = Command::new("rustc").arg("-vV").output()?;
            let compiler = String::from_utf8_lossy(&rustc.stdout);
            let kind = match args.get(3).map(String::as_str) {
                None | Some("decoder") => identity::Kind::Decoder,
                Some("image-path") => identity::Kind::ImagePath,
                Some(other) => return Err(format!("unknown identity kind {other}").into()),
            };
            let files = identity::files(root, kind)?;
            println!("{}", identity::hash(root, &files, &compiler, flags)?);
            Ok(ExitCode::SUCCESS)
        }
        "format" => {
            let samples = ledger::lines(&fs::read_to_string(arg(2)?)?)?;
            let crashes = ledger::lines(&fs::read_to_string(arg(3)?)?)?;
            println!("{}", ledger::format_entry(arg(1)?, &samples, &crashes)?);
            Ok(ExitCode::SUCCESS)
        }
        "escape" => {
            let (passed, total) = escape::counts(&fs::read_to_string(arg(1)?)?, "g4_sandbox")
                .ok_or("no g4_sandbox result in the log")?;
            println!(
                "{}",
                json!({"revision": arg(2)?, "passed": passed, "total": total})
            );
            Ok(ExitCode::SUCCESS)
        }
        "miri-check" => {
            let fresh: Value = serde_json::from_str(&fs::read_to_string(arg(1)?)?)?;
            if let Some(committed) = args.get(2) {
                let committed: Value = serde_json::from_str(&fs::read_to_string(committed)?)?;
                miri::same_results(&fresh, &committed)?;
                println!("the fresh Miri record matches the committed one");
            } else {
                miri::validate(&fresh)?;
                println!("the Miri record is well formed");
            }
            Ok(ExitCode::SUCCESS)
        }
        "gate-status" => {
            let samples = ledger::lines(&fs::read_to_string(arg(1)?)?)?;
            let crashes = ledger::lines(&fs::read_to_string(arg(2)?)?)?;
            let st = match (args.get(3), args.get(4)) {
                (Some(min), Some(window)) => {
                    gate::status_with(&samples, &crashes, min.parse()?, window.parse()?)?
                }
                _ => gate::status(&samples, &crashes)?,
            };
            println!(
                "{}",
                json!({
                    "cpu_hours": st.cpu_hours, "edges": st.edges, "crashes": st.crashes,
                    "recent_crashes": st.recent_crashes,
                    "hours_since_new_crash": st.hours_since_new_crash,
                    "hours_since_coverage_gain": st.hours_since_coverage_gain,
                    "plateau": st.plateau, "holds": st.holds,
                })
            );
            Ok(ExitCode::SUCCESS)
        }
        "slice-seconds" => {
            // SLICE WORKERS CAP_HOURS DONE_HOURS RATE|- WALL_LEFT|-
            let num = |i: usize| -> Result<f64, Box<dyn Error>> { Ok(arg(i)?.parse()?) };
            let opt = |i: usize| -> Result<Option<f64>, Box<dyn Error>> {
                Ok(if arg(i)? == "-" {
                    None
                } else {
                    Some(arg(i)?.parse()?)
                })
            };
            let seconds = slice::next_slice_seconds(
                num(1)?,
                arg(2)?.parse()?,
                num(3)?,
                num(4)?,
                opt(5)?,
                opt(6)?,
            );
            println!("{seconds}");
            Ok(ExitCode::SUCCESS)
        }
        "learn-rate" => {
            // PREVIOUS|- CPU_SECONDS WORKERS WALL_SECONDS
            let previous = if arg(1)? == "-" {
                None
            } else {
                Some(arg(1)?.parse()?)
            };
            println!(
                "{:.4}",
                slice::learn_rate(
                    previous,
                    arg(2)?.parse()?,
                    arg(3)?.parse()?,
                    arg(4)?.parse()?
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        "annotate" => {
            let ledger: Value = serde_json::from_str(&fs::read_to_string(arg(1)?)?)?;
            let mut notes = serde_json::Map::new();
            // NAME=FILE pairs: each file's JSON becomes notes[NAME].
            for pair in &args[2..] {
                let (name, file) = pair.split_once('=').ok_or("expected NAME=FILE")?;
                notes.insert(
                    name.to_string(),
                    serde_json::from_str(&fs::read_to_string(file)?)?,
                );
            }
            println!("{}", ledger::with_notes(ledger, Value::Object(notes)));
            Ok(ExitCode::SUCCESS)
        }
        "ledger" => {
            let esc: Value = serde_json::from_str(arg(4)?)?;
            let formats = args[5..]
                .iter()
                .map(|f| serde_json::from_str(f))
                .collect::<Result<Vec<Value>, _>>()?;
            let n = |k: &str| {
                esc[k]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or(format!("escape json has no {k}"))
            };
            println!(
                "{}",
                ledger::ledger(
                    arg(1)?,
                    arg(2)?,
                    arg(3)?,
                    &formats,
                    esc["revision"]
                        .as_str()
                        .ok_or("escape json has no revision")?,
                    n("passed")?,
                    n("total")?
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command {other}").into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("decoder-fuzz-tools: {e}");
            ExitCode::from(2)
        }
    }
}
