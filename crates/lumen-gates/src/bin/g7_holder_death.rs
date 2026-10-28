//! Gate G7: when the terminal exits, every holder exits, even if a shell is
//! still alive and even if other threads spawn processes while the holder is
//! created.
//!
//! The terminal gives each holder the read end of an EOF pipe as its stdin and
//! keeps the write end. When the terminal dies the write end closes and the
//! holder reads EOF. The pipe only works if no other process inherited the
//! write end, so the stress scenario spawns processes from other threads while
//! the pipe is created: on a platform where pipe creation and close-on-exec
//! are two steps, one of those processes can inherit the write end and keep
//! the holder alive.
//!
//! Run with no arguments: the harness starts a terminal, kills it with
//! SIGKILL and checks the holder. `terminal` and `holder` are internal roles.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, io, thread};

const STRESS_THREADS: usize = 4;
/// Holders created, each with its own EOF pipe, in the stress scenario.
const STRESS_HOLDERS: usize = 100;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("holder") => holder(args.get(2).map_or("", String::as_str)),
        Some("terminal") => terminal(args.get(2).map_or("", String::as_str)),
        _ => harness(),
    }
}

/// The holder: block until stdin reaches EOF, record it, exit.
fn holder(log: &str) -> ExitCode {
    let mut sink = [0u8; 64];
    let mut stdin = io::stdin().lock();
    loop {
        match stdin.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).open(log) {
        let _ = writeln!(
            f,
            "holder pid {} saw EOF and is exiting",
            std::process::id()
        );
    }
    ExitCode::SUCCESS
}

/// The terminal: spawn a shell and holders from the main thread, report their
/// pids, then wait to be killed.
fn terminal(mode: &str) -> ExitCode {
    let stress = mode == "--stress";
    let log = env::var("G7_LOG").unwrap_or_default();
    if stress {
        for _ in 0..STRESS_THREADS {
            thread::spawn(|| {
                loop {
                    // Children that outlive the check window, so a leaked write
                    // end would keep a holder alive past it.
                    if let Ok(child) = Command::new("sleep").arg("7").stdin(Stdio::null()).spawn() {
                        drop(child);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
    // Negative control: hand the write end to the "shell", as a leak would.
    let leaked = if mode == "--leak" {
        io::pipe().ok()
    } else {
        None
    };
    let shell_stdin = leaked
        .as_ref()
        .and_then(|(_, w)| w.try_clone().ok())
        .map_or_else(Stdio::null, Stdio::from);
    let Ok(shell) = Command::new("sleep").arg("600").stdin(shell_stdin).spawn() else {
        return ExitCode::FAILURE;
    };
    let Ok(exe) = env::current_exe() else {
        return ExitCode::FAILURE;
    };
    println!("shell {}", shell.id());
    let holders = if stress { STRESS_HOLDERS } else { 1 };
    let mut write_ends = Vec::new();
    for _ in 0..holders {
        let pipe = match leaked.as_ref() {
            Some((r, w)) => r.try_clone().and_then(|r| w.try_clone().map(|w| (r, w))),
            None => io::pipe(),
        };
        let Ok((reader, writer)) = pipe else {
            return ExitCode::FAILURE;
        };
        let Ok(holder) = Command::new(&exe)
            .args(["holder", &log])
            .stdin(reader)
            .spawn()
        else {
            return ExitCode::FAILURE;
        };
        println!("holder {}", holder.id());
        write_ends.push(writer);
    }
    println!("ready");
    let _ = io::stdout().flush();
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn read_pids(child: &mut Child) -> Option<(u32, Vec<u32>)> {
    let out = child.stdout.take()?;
    let (mut shell, mut holders) = (None, Vec::new());
    for line in BufReader::new(out).lines() {
        let line = line.ok()?;
        match line.split_once(' ') {
            Some(("shell", p)) => shell = p.parse().ok(),
            Some(("holder", p)) => holders.push(p.parse().ok()?),
            _ if line == "ready" => break,
            _ => {}
        }
    }
    Some((shell?, holders))
}

/// One scenario; returns true when every holder died after the terminal was killed.
fn scenario(name: &str, mode: &str) -> bool {
    let log = env::temp_dir().join(format!("g7-{}-{name}.log", std::process::id()));
    let _ = fs::remove_file(&log);
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("terminal")
        .env("G7_LOG", &log)
        .stdout(Stdio::piped());
    cmd.arg(mode);
    let Ok(mut term) = cmd.spawn() else {
        println!("G7 {name}: could not start the terminal");
        return false;
    };
    let Some((shell, holders)) = read_pids(&mut term) else {
        println!("G7 {name}: terminal did not report pids");
        let _ = term.kill();
        return false;
    };
    let before = holders.iter().filter(|p| alive(**p)).count();
    println!(
        "G7 {name}: terminal={} shell={shell} holders={} before kill: shell alive={} holders alive={before}",
        term.id(),
        holders.len(),
        alive(shell)
    );
    let _ = term.kill(); // SIGKILL: no cleanup runs in the terminal
    let _ = term.wait();
    let deadline = Instant::now() + Duration::from_secs(5);
    while holders.iter().any(|p| alive(*p)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let survivors: Vec<u32> = holders.iter().copied().filter(|p| alive(*p)).collect();
    println!(
        "G7 {name}: after kill: shell alive={} holders alive={} {survivors:?}",
        alive(shell),
        survivors.len()
    );
    let saw = fs::read_to_string(&log).unwrap_or_default().lines().count();
    println!(
        "G7 {name}: holders that logged EOF: {saw} of {}",
        holders.len()
    );
    let mut kill = Command::new("kill");
    kill.arg("-9").arg(shell.to_string());
    for p in &survivors {
        kill.arg(p.to_string());
    }
    let _ = kill.stderr(Stdio::null()).status();
    let _ = fs::remove_file(&log);
    before == holders.len() && survivors.is_empty() && saw == holders.len()
}

fn harness() -> ExitCode {
    println!("G7 os={} arch={}", env::consts::OS, env::consts::ARCH);
    let plain = scenario("plain", "--plain");
    let stress = scenario("stress", "--stress");
    // The control leaks the write end to the shell on purpose, so a holder must
    // survive. If it does not, this harness cannot detect a leak and the other
    // results mean nothing.
    let control_detects_leak = !scenario("control", "--leak");
    println!("G7 plain: {}", if plain { "ok" } else { "FAILED" });
    println!("G7 stress: {}", if stress { "ok" } else { "FAILED" });
    println!(
        "G7 control (deliberate leak must be caught): {}",
        if control_detects_leak { "ok" } else { "FAILED" }
    );
    if plain && stress && control_detects_leak {
        println!("G7 PASS");
        ExitCode::SUCCESS
    } else {
        println!("G7 FAIL");
        ExitCode::FAILURE
    }
}
