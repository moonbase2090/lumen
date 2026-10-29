//! `decoder-fuzz-tools`: command line over the library.
//!
//! - `classify LOG` prints the crash kind of a libFuzzer log and exits 1 if it
//!   reports a crash, 0 if not.
//! - `identity ROOT [RUSTFLAGS]` prints the decoder build identity.
//! - `seeds DIR` writes the seed images under `DIR/{png,jpeg,webp}`.
//! - `format NAME SAMPLES CRASHES` prints one format's ledger entry.
//! - `escape TESTLOG REVISION` prints `{revision, passed, total}` for the G4
//!   suite from a `cargo test` log.
//! - `ledger REVISION DECODER_BUILD IMAGE_PATH_BUILD ESCAPE_JSON ENTRY_JSON...`
//!   prints the ledger.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use lumen_decoder_tools::{classify, escape, identity, ledger, seeds};
use serde_json::{Value, json};

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
        "identity" => {
            let root = Path::new(arg(1)?);
            let flags = args.get(2).map_or("", String::as_str);
            let rustc = Command::new("rustc").arg("-vV").output()?;
            let compiler = String::from_utf8_lossy(&rustc.stdout);
            let files = identity::decoder_files(root)?;
            println!("{}", identity::hash(root, &files, &compiler, flags)?);
            Ok(ExitCode::SUCCESS)
        }
        "seeds" => {
            for (name, bytes) in seeds::all()? {
                let p = Path::new(arg(1)?).join(name);
                if let Some(d) = p.parent() {
                    fs::create_dir_all(d)?;
                }
                fs::write(p, bytes)?;
            }
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
            println!("{}", json!({"revision": arg(2)?, "passed": passed, "total": total}));
            Ok(ExitCode::SUCCESS)
        }
        "ledger" => {
            let esc: Value = serde_json::from_str(arg(4)?)?;
            let formats = args[5..]
                .iter()
                .map(|f| serde_json::from_str(f))
                .collect::<Result<Vec<Value>, _>>()?;
            let n = |k: &str| esc[k].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or(format!("escape json has no {k}"));
            println!(
                "{}",
                ledger::ledger(arg(1)?, arg(2)?, arg(3)?, formats, esc["revision"].as_str().ok_or("escape json has no revision")?, n("passed")?, n("total")?)
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
