//! `lumen-term`: the Lumen terminal.
//!
//! With the `window` feature it opens a window. With `--snapshot FILE` it runs
//! the program headless, waits, and writes a PNG of what the window would show,
//! which is also how a frame is captured on a machine with no display.

use lumen_term::App;
use lumen_term::cli::{Options, USAGE};
use std::process::ExitCode;
use std::time::Instant;

fn layer() -> Box<dyn lumen_term::Layer + Send> {
    Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }))
}

fn command(o: &Options) -> (String, Vec<String>) {
    match o.command.split_first() {
        Some((p, a)) => (p.clone(), a.to_vec()),
        None => (
            std::env::var("SHELL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/sh".to_owned()),
            Vec::new(),
        ),
    }
}

fn snapshot(o: &Options, path: &str) -> Result<(), String> {
    let (program, arguments) = command(o);
    let mut app = App::start(
        layer(),
        &program,
        &arguments,
        o.scale.unwrap_or(1),
        o.size.0,
        o.size.1,
    )
    .map_err(|e| e.to_string())?;
    let end = Instant::now() + o.wait;
    while Instant::now() < end && !app.has_exited() {
        app.pump();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Take what the program wrote as it exited.
    for _ in 0..5 {
        app.pump();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let view = app.view();
    let png = view.to_png().map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| format!("{path}: {e}"))?;
    println!("{}x{} -> {path}", view.width, view.height);
    Ok(())
}

#[cfg(feature = "window")]
fn window(o: &Options) -> Result<(), String> {
    lumen_term::host::run(lumen_term::host::Config {
        layer: Box::new(layer),
        command: o.command.clone(),
        size: o.size,
        scale: o.scale,
        exit_after: o.exit_after,
    })
}

#[cfg(not(feature = "window"))]
fn window(_: &Options) -> Result<(), String> {
    Err("this build has no window: build with --features window, or use --snapshot".into())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match Options::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match &options.snapshot {
        Some(path) => snapshot(&options, path),
        None => window(&options),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lumen-term: {e}");
            ExitCode::from(1)
        }
    }
}
