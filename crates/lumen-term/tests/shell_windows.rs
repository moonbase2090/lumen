//! On Windows: the shell chosen with no setting and no `SHELL` is PowerShell,
//! and the window's own session can start it and read what it writes. This
//! runs only on Windows; elsewhere the order is covered by `shell.rs`.

#![cfg(windows)]

use lumen_term::shell::{choose, in_path};
use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

#[test]
fn the_default_on_windows_is_powershell_and_the_window_can_run_it() {
    let comspec = std::env::var("COMSPEC").ok();
    let program = choose(None, None, true, comspec.as_deref(), in_path);
    assert!(
        program == "pwsh.exe" || program == "powershell.exe",
        "PowerShell is installed here, so it is chosen; got {program}"
    );
    // `lumen$(1+1)ok` prints `lumen2ok`; the command line itself never contains that.
    let mut app = App::start(
        Box::new(Passthrough),
        &program,
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Write-Output lumen$(1+1)ok",
        ],
        1,
        640,
        400,
    )
    .unwrap_or_else(|e| unreachable!("{program} starts: {e}"));
    let end = Instant::now() + Duration::from_secs(60);
    let mut seen = false;
    while Instant::now() < end && !seen {
        app.pump();
        seen = app.session().document().blocks().iter().any(|b| match b {
            Block::Grid(g) => g.text_rows().iter().any(|r| r.contains("lumen2ok")),
            Block::Rich(_) => false,
        });
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(seen, "the window showed what {program} wrote");
}
