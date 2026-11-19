//! A program that asks where the cursor is is told, with no feature turned on.

#![cfg(unix)]

use lumen_term::{Block, Layer, Passthrough, Session};
use std::time::{Duration, Instant};

fn b_layer() -> Box<dyn Layer + Send> {
    Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }))
}

fn grid_text(s: &Session) -> String {
    s.document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run a program that prints `queries` and then reads exactly `reply_len` bytes
/// in raw mode, waiting for them to arrive, and return what it received.
fn ask(layer: Box<dyn Layer + Send>, name: &str, queries: &[u8], reply_len: usize) -> Vec<u8> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let q = dir.join(format!("{name}.queries"));
    let out = dir.join(format!("{name}.received"));
    assert!(std::fs::write(&q, queries).is_ok());
    let _ = std::fs::remove_file(&out);
    let script = format!(
        "stty raw -echo; cat '{q}'; \
         dd bs=1 count={reply_len} of='{out}' 2>/dev/null; echo DONE",
        q = q.display(),
        out = out.display()
    );
    let mut s = Session::start(layer, 60, 6, "/bin/sh", &["-c", &script])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !grid_text(&s).contains("DONE") {
        s.pump_wait(Duration::from_millis(100));
    }
    assert!(
        grid_text(&s).contains("DONE"),
        "{name}: the program never finished; the grid shows {:?}",
        grid_text(&s)
    );
    std::fs::read(&out).unwrap_or_default()
}

#[test]
fn a_program_that_asks_where_the_cursor_is_is_told_on_every_layer() {
    for (label, layer) in [
        (
            "passthrough",
            Box::new(Passthrough) as Box<dyn Layer + Send>,
        ),
        ("arm-b", b_layer()),
    ] {
        let got = ask(
            layer,
            &format!("cursor-{label}"),
            b"ab\r\n  x\x1b[5n\x1b[c\x1b[6n",
            6,
        );
        assert_eq!(
            String::from_utf8_lossy(&got),
            "\x1b[2;4R",
            "{label}: the first thing read is the cursor position, so nothing else was answered before it"
        );
    }
}
