//! The input box's status row says where the keys go: to the shell when the
//! shell reads them, and to the program in front by name when one does,
//! however that program was started.

#![cfg(unix)]

use lumen_term::{App, Passthrough};
use std::time::{Duration, Instant};

fn start(program: &str, arguments: &[&str]) -> App {
    App::start(Box::new(Passthrough), program, arguments, 1, 800, 400)
        .unwrap_or_else(|e| unreachable!("{program} starts: {e}"))
}

fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        app.pump();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn status(app: &App) -> String {
    app.input_status().unwrap_or_default()
}

/// A program started directly is in front of its own terminal, and is named:
/// it is not "the shell" because nothing else is in front of it.
#[test]
fn a_program_started_directly_is_named_not_called_the_shell() {
    let mut app = start("/bin/sleep", &["60"]);
    assert!(
        pump_until(&mut app, |a| status(a) == "Keys go to sleep"),
        "status: {:?}",
        status(&app)
    );
}
