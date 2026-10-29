//! `lumen-term`: the Lumen terminal.
//!
//! With the `window` feature it opens a window. With `--snapshot FILE` it runs
//! the program headless, waits, and writes a PNG of what the window would show,
//! which is also how a frame is captured on a machine with no display.

use lumen_term::App;
use lumen_term::cli::{Options, USAGE};
use lumen_term::policy::Policy;
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

fn snapshot(o: &Options, policy: &Policy, path: &str) -> Result<(), String> {
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
    app.use_settings(policy.settings());
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
fn window(o: &Options, policy: &Policy) -> Result<(), String> {
    lumen_term::host::run(lumen_term::host::Config {
        layer: Box::new(layer),
        command: o.command.clone(),
        size: o.size,
        scale: o.scale,
        exit_after: o.exit_after,
        notifications: policy.settings(),
    })
}

#[cfg(not(feature = "window"))]
fn window(_: &Options, _: &Policy) -> Result<(), String> {
    Err("this build has no window: build with --features window, or use --snapshot".into())
}

fn describe(policy: &Policy) -> String {
    match policy {
        Policy::Defaults => "policy: no file; every setting is at its default (all off)".to_owned(),
        Policy::Loaded { path, settings } => format!(
            "policy: {} allow_unsigned={} os_notifications={} os_text={}",
            path.display(),
            settings.allow_unsigned,
            settings.os_notifications,
            settings.os_text
        ),
        Policy::Ignored { .. } => format!("policy: {}", policy.notice().unwrap_or_default()),
    }
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
    let policy = lumen_term::policy::load();
    if options.show_policy {
        println!("{}", describe(&policy));
        return ExitCode::SUCCESS;
    }
    // The terminal's own notice: it is not from the program and a program
    // cannot reach it. A window will show it as chrome once chrome notices are drawn.
    if let Some(notice) = policy.notice() {
        eprintln!("lumen-term: {notice}");
    }
    let result = match &options.snapshot {
        Some(path) => snapshot(&options, &policy, path),
        None => window(&options, &policy),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lumen-term: {e}");
            ExitCode::from(1)
        }
    }
}
