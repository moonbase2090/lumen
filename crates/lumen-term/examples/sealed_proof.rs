//! A driver for the sealed-shell proof on Linux: it starts a listed zsh through
//! the terminal's own session path with the real sealing conditions, types one
//! command, and prints what came back. Copy it next to `lumen-holder` (the
//! terminal looks for the holder beside its executable) and run it.
//!
//! `sealed_proof [PATH-TO-ZSH]` prints the sealing status, the sealed frames
//! the document received, the holder's hardening and whether the holder ended
//! with the shell. Exit status 0 if the layer was sealed and the command
//! produced its start, end and prompt frames; 1 otherwise.

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    use lumen_term::seal::Status;
    use lumen_term::{Event, Session};
    use std::time::{Duration, Instant};

    let shell = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/usr/bin/zsh".to_owned());
    let layer = Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }));
    let no_arguments: [&str; 0] = [];
    let mut session = match Session::start(layer, 100, 30, &shell, &no_arguments) {
        Ok(s) => s,
        Err(e) => {
            println!("could not start: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    println!("status: {:?}", session.seal_status());
    let holder = session.holder_pid();
    println!("holder running: {}", holder.is_some());
    let mut frames = Vec::new();
    let pump = |session: &mut Session, frames: &mut Vec<(u8, usize)>, seconds: u64| {
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            session.pump_wait(Duration::from_millis(100));
            for event in session.take_events() {
                if let Event::Sealed { kind, body } = event {
                    frames.push((kind, body.len()));
                }
            }
        }
    };
    pump(&mut session, &mut frames, 3);
    // Read after the holder has had time to harden itself.
    if let Some(pid) = holder {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        for key in ["Name:", "NoNewPrivs:", "Seccomp:", "CapEff:"] {
            if let Some(line) = status.lines().find(|l| l.starts_with(key)) {
                println!("holder {line}");
            }
        }
    }
    let _ = session.send(b"echo sealed-proof\n");
    pump(&mut session, &mut frames, 3);
    println!("sealed frames (type, payload bytes): {frames:?}");
    let _ = session.send(b"exit\n");
    pump(&mut session, &mut frames, 3);
    let ended = holder.is_none_or(|pid| {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .is_none_or(|t| {
                t.rsplit(')')
                    .next()
                    .is_some_and(|r| r.trim_start().starts_with('Z'))
            })
    });
    println!("holder ended with the shell: {ended}");
    let kinds: Vec<u8> = frames.iter().map(|(k, _)| *k).collect();
    let ok = session.seal_status() == Status::Sealed
        && kinds.contains(&0x10)
        && kinds.contains(&0x12)
        && kinds.contains(&0x11)
        && ended;
    std::process::ExitCode::from(u8::from(!ok))
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("the sealed shell is Linux only");
}
