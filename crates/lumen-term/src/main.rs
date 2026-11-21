//! `bright`: the Lumen reference terminal.
//!
//! With the `window` feature it opens a window. With `--snapshot FILE` it runs
//! the program headless, waits, and writes a PNG of what the window would show,
//! which is also how a frame is captured on a machine with no display.

// With the window on Windows this is a program with a window and no console of
// its own, so no black console window opens behind it. The cost is that the
// text the command line options print (usage, --show-settings) only reaches a
// terminal if the output is redirected.
#![cfg_attr(all(windows, feature = "window"), windows_subsystem = "windows")]

use lumen_term::App;
use lumen_term::cli::{Options, USAGE};
use lumen_term::policy::Policy;
use lumen_term::settings::{Look, Settings, Watcher as SettingsWatcher};
use std::process::ExitCode;
use std::time::Instant;

fn layer() -> Box<dyn lumen_term::Layer + Send> {
    Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }))
}

/// The program to run: the one given on the command line, else the shell the
/// settings name, else the user's own (`$SHELL`).
fn command(o: &Options, look: &Look) -> (String, Vec<String>) {
    match o.command.split_first() {
        Some((p, a)) => (p.clone(), a.to_vec()),
        None => (
            lumen_term::shell::default_shell(look.shell.as_deref()),
            Vec::new(),
        ),
    }
}

fn snapshot(o: &Options, policy: &Policy, look: &Look, path: &str) -> Result<(), String> {
    let (program, arguments) = command(o, look);
    let mut app = App::start_with_shell(
        layer(),
        &program,
        &arguments,
        o.scale.unwrap_or(1),
        o.size.0,
        o.size.1,
        lumen_term::shellint::ShellOptions {
            integration: look.shell_integration,
        },
    )
    .map_err(|e| e.to_string())?;
    app.use_settings(policy.settings());
    app.use_images(policy.images());
    // There is no system appearance without a window: the light one stands.
    app.set_theme_choice(look.theme);
    app.set_fonts(&look.fonts).map_err(|e| e.to_string())?;
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
fn window(o: &Options, policy: &Policy, settings: SettingsWatcher) -> Result<(), String> {
    lumen_term::host::run(lumen_term::host::Config {
        layer: Box::new(layer),
        command: o.command.clone(),
        size: o.size,
        scale: o.scale,
        exit_after: o.exit_after,
        notifications: policy.settings(),
        images: policy.images(),
        settings,
    })
}

#[cfg(not(feature = "window"))]
fn window(_: &Options, _: &Policy, _: SettingsWatcher) -> Result<(), String> {
    Err("this build has no window: build with --features window, or use --snapshot".into())
}

fn describe_settings(settings: &Settings, look: &Look) -> String {
    let source = match settings {
        Settings::Defaults => "no file; the defaults apply".to_owned(),
        Settings::Loaded { path, .. } => path.display().to_string(),
        Settings::Ignored { .. } => settings.notice().unwrap_or_default(),
    };
    let s = settings.look();
    let face = |f: &lumen_term::settings::FontSetting| match f {
        lumen_term::settings::FontSetting::Bundled => "bundled".to_owned(),
        lumen_term::settings::FontSetting::File(p) => p.display().to_string(),
    };
    let theme = match &s.theme {
        lumen_term::settings::ThemeSetting::System => "system".to_owned(),
        lumen_term::settings::ThemeSetting::Builtin(b) => b.name().to_owned(),
        lumen_term::settings::ThemeSetting::File(p) => p.display().to_string(),
    };
    let mut out = format!(
        "settings: {source}\ntheme={theme} mono_font={} mono_size={} prose_font={} prose_size={} copy_on_select={} smooth_scroll={} shell={} shell_integration={}",
        face(&s.mono_font),
        look.fonts.mono_px(),
        face(&s.prose_font),
        look.fonts.prose_px(),
        s.copy_on_select,
        s.smooth_scroll,
        s.shell.as_deref().unwrap_or("default"),
        s.shell_integration
    );
    for notice in &look.notices {
        out.push('\n');
        out.push_str(notice);
    }
    out
}

fn describe(policy: &Policy) -> String {
    match policy {
        Policy::Defaults => "policy: no file; every setting is at its default (all off)".to_owned(),
        Policy::Loaded {
            path,
            settings,
            images,
        } => format!(
            "policy: {} allow_unsigned={} os_notifications={} os_text={} images={}",
            path.display(),
            settings.allow_unsigned,
            settings.os_notifications,
            settings.os_text,
            images
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
        eprintln!("bright: {notice}");
    }
    let settings = SettingsWatcher::system();
    let look = settings.current().look().resolve();
    if options.show_settings {
        println!("{}", describe_settings(settings.current(), &look));
        return ExitCode::SUCCESS;
    }
    for notice in settings.current().notice().iter().chain(&look.notices) {
        eprintln!("bright: {notice}");
    }
    let result = match &options.snapshot {
        Some(path) => snapshot(&options, &policy, &look, path),
        None => window(&options, &policy, settings),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bright: {e}");
            ExitCode::from(1)
        }
    }
}
