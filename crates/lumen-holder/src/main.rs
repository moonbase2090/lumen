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
    use lumen_holder::principal::Principal;
    use lumen_holder::run::{FAULT, harden, run};
    const USAGE: &str = "usage: lumen-holder --terminal PID --shell-pid PID --shell-start TICKS [--report]\n       lumen-holder --terminal PID --check-hardening";
    let mut args = std::env::args().skip(1);
    let (mut terminal, mut shell_pid, mut shell_start) = (None, None, None);
    let (mut check, mut report) = (false, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--terminal" => terminal = args.next().and_then(|v| v.parse::<u32>().ok()),
            "--shell-pid" => shell_pid = args.next().and_then(|v| v.parse::<u32>().ok()),
            "--shell-start" => shell_start = args.next().and_then(|v| v.parse::<u64>().ok()),
            "--check-hardening" => check = true,
            "--report" => report = true,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(terminal) = terminal else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if check {
        return match harden(terminal, &[1]) {
            Ok(()) => {
                report_hardening();
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("lumen-holder: cannot harden: {e}");
                ExitCode::from(FAULT)
            }
        };
    }
    let (Some(pid), Some(start_time)) = (shell_pid, shell_start) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let shell = Principal {
        pid,
        start_time,
        uid: lumen_holder::ffi_linux::effective_uid(),
    };
    match run(terminal, shell, report) {
        Ok((end, counters)) => {
            if report {
                println!(
                    "ready={} second_ready={} refused={} malformed={}",
                    counters.ready, counters.second_ready, counters.refused, counters.malformed
                );
            }
            ExitCode::from(end.code())
        }
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
fn report_hardening() {
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
