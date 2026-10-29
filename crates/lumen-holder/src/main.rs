//! The key-holder process: `lumen-holder --terminal PID`.
//!
//! Started by the terminal with four descriptors (see the library docs). With
//! `--check-hardening` it hardens itself, then tries to do what hardening
//! forbids and prints what happened, which is how the hardening is shown on a
//! real system.

use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("lumen-holder runs on Linux only; shell integration is not sealed elsewhere");
    ExitCode::from(2)
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    use lumen_holder::run::{FAULT, harden, run};
    let mut args = std::env::args().skip(1);
    let mut terminal = None;
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--terminal" => terminal = args.next().and_then(|v| v.parse::<u32>().ok()),
            "--check-hardening" => check = true,
            _ => {
                eprintln!("usage: lumen-holder --terminal PID [--check-hardening]");
                return ExitCode::from(2);
            }
        }
    }
    let Some(terminal) = terminal else {
        eprintln!("usage: lumen-holder --terminal PID [--check-hardening]");
        return ExitCode::from(2);
    };
    if check {
        return match harden(terminal, &[1]) {
            Ok(()) => {
                report();
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("lumen-holder: cannot harden: {e}");
                ExitCode::from(FAULT)
            }
        };
    }
    match run(terminal) {
        Ok(end) => ExitCode::from(end.code()),
        Err(e) => {
            eprintln!("lumen-holder: {e}");
            ExitCode::from(FAULT)
        }
    }
}

/// What the hardened process can and cannot do, printed one fact per line.
/// The exec comes last: if it worked, the process would be replaced and the
/// line would never be printed.
#[cfg(target_os = "linux")]
fn report() {
    use std::io::Write as _;
    use std::os::unix::process::CommandExt as _;
    let describe = |r: std::io::Result<()>| match r {
        Ok(()) => "allowed".to_owned(),
        Err(e) => format!("refused ({:?})", e.kind()),
    };
    println!("dumpable={:?}", lumen_holder::ffi_linux::dumpable().ok());
    let spawn = std::process::Command::new("/bin/true").spawn().map(drop);
    println!("spawn={}", describe(spawn));
    let thread = std::thread::Builder::new().spawn(|| ()).map(drop);
    println!("thread={}", describe(thread));
    // The listing's own descriptor is not one the holder keeps.
    let listing = format!("/proc/{}/fd", std::process::id());
    let mut fds: Vec<String> = std::fs::read_dir("/proc/self/fd")
        .map(|d| {
            d.filter_map(|e| {
                let name = e.ok()?.file_name().to_str()?.to_owned();
                let target = std::fs::read_link(format!("/proc/self/fd/{name}")).ok()?;
                (target.to_str() != Some(listing.as_str())).then_some(name)
            })
            .collect()
        })
        .unwrap_or_default();
    fds.sort();
    println!("fds={}", fds.join(","));
    let _ = std::io::stdout().flush();
    let refused = std::process::Command::new("/bin/true").exec();
    println!("exec=refused ({:?})", refused.kind());
}
