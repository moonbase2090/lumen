//! Show one real operating-system notification through the backend and print
//! what was started, to see the platform's own program accept it.
//!
//! `cargo run -p lumen-term --example os_notify -- "text"`

use lumen_term::notify::{OsNotification, OsSink, escape_markup};
use lumen_term::os_backend::{Platform, SystemSink, invocation};

fn main() {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "backend check".to_owned());
    let note = OsNotification {
        layer: 1,
        text: escape_markup(&format!("Program in tab 1 says: {text}")),
        token: 1,
    };
    let Some(platform) = Platform::current() else {
        println!("no backend on this platform");
        return;
    };
    let i = invocation(platform, &note);
    println!("platform: {platform:?}");
    println!("program: {}", i.program.display());
    println!("arguments: {:?}", i.args);
    let mut sink = SystemSink::new();
    sink.send(&note);
    // The launch is detached; give the short-lived program a moment to finish.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    println!("shown={} failed={}", sink.shown(), sink.failed());
}
