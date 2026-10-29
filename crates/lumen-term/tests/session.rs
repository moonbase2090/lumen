//! A real program on a real PTY, through the arm layer, into the document.

#![cfg(unix)]

use lumen_detector::TAG;
use lumen_term::{Block, Layer, Passthrough, Session};
use std::time::{Duration, Instant};

/// A Track 0 paragraph frame whose bytes survive a terminal line discipline
/// (no line feed or carriage return in it).
fn paragraph(text: &str) -> Vec<u8> {
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(0x80);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend(body);
    assert!(!v.contains(&b'\n') && !v.contains(&b'\r'));
    v
}

fn file(name: &str, bytes: &[u8]) -> String {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    assert!(std::fs::write(&path, bytes).is_ok());
    path.to_string_lossy().into_owned()
}

fn pump_until(s: &mut Session, done: impl Fn(&Session) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        s.pump_wait(Duration::from_millis(100));
        if done(s) {
            return true;
        }
    }
    false
}

fn a_layer(sealed: bool) -> Box<dyn Layer + Send> {
    let state = if sealed {
        lumen_arm_a::LayerState::Sealed
    } else {
        lumen_arm_a::LayerState::Unsealed
    };
    Box::new(lumen_arm_a::pipeline::Layer::new(
        lumen_arm_a::pipeline::Config {
            budget: lumen_arm_a::pipeline::Budget::quarter_of(2000, 8 << 20),
        },
        state,
    ))
}

fn b_layer() -> Box<dyn Layer + Send> {
    Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }))
}

fn kinds(s: &Session) -> Vec<&'static str> {
    s.document()
        .blocks()
        .iter()
        .map(|b| match b {
            Block::Grid(_) => "grid",
            Block::Rich(r) if r.background => "background",
            Block::Rich(_) => "rich",
        })
        .collect()
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

fn start(layer: Box<dyn Layer + Send>, cols: usize, rows: usize, script: &str) -> Session {
    Session::start(layer, cols, rows, "/bin/sh", &["-c", script])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

#[test]
fn a_programs_output_reaches_the_grid_through_the_layer() {
    let mut s = start(Box::new(Passthrough), 30, 5, "echo hi from sh");
    assert!(pump_until(&mut s, |s| grid_text(s).contains("hi from sh")));
}

#[test]
fn the_same_bytes_from_a_real_program_make_a_block_in_arm_b_and_on_a_sealed_arm_a_layer() {
    let f = file("frame-block.bin", &paragraph("hello"));
    for (name, layer) in [("arm B", b_layer()), ("arm A sealed", a_layer(true))] {
        let mut s = start(layer, 30, 5, &format!("echo before; cat {f}; echo after"));
        assert!(
            pump_until(&mut s, |s| grid_text(s).contains("after")),
            "{name}"
        );
        s.finish();
        assert_eq!(kinds(&s), ["grid", "rich", "grid"], "{name}");
    }
}

#[test]
fn arm_a_on_an_unsealed_layer_shows_the_same_bytes_as_text_and_makes_no_block() {
    let f = file("frame-unsealed.bin", &paragraph("hello"));
    let mut s = start(
        a_layer(false),
        60,
        5,
        &format!("echo before; cat {f}; echo after"),
    );
    assert!(pump_until(&mut s, |s| grid_text(s).contains("after")));
    s.finish();
    assert_eq!(kinds(&s), ["grid"]);
}

#[test]
fn the_alternate_screen_of_the_real_program_reaches_the_layer() {
    let f = file("frame-alt.bin", &paragraph("hello"));
    for (name, layer) in [("arm B", b_layer()), ("arm A sealed", a_layer(true))] {
        // The switch is a separate read from the frame, so the layer has seen
        // the alternate screen before the frame arrives.
        let mut s = start(
            layer,
            60,
            5,
            &format!("printf '\\033[?1049h'; sleep 1; cat {f}; sleep 1"),
        );
        assert!(
            pump_until(&mut s, |s| s
                .document()
                .blocks()
                .iter()
                .any(|b| matches!(b, Block::Grid(g) if g.alt_active()))),
            "{name} saw the alternate screen"
        );
        let mut seen_frame = false;
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && !seen_frame {
            s.pump_wait(Duration::from_millis(100));
            seen_frame = grid_text(&s).contains("unverified") || grid_text(&s).contains("hello");
        }
        s.finish();
        assert!(
            kinds(&s).iter().all(|k| *k == "grid"),
            "{name}: on the alternate screen the frame is text, not a block: {:?}",
            kinds(&s)
        );
    }
}

#[test]
fn resizing_the_session_resizes_every_block_and_the_programs_pty() {
    let mut s = start(
        Box::new(Passthrough),
        30,
        7,
        "echo first; read x; stty size",
    );
    assert!(pump_until(&mut s, |s| grid_text(s).contains("first")));
    assert!(s.resize(50, 12).is_ok());
    assert_eq!(s.document().columns(), 50);
    assert_eq!(s.document().rows(), 12);
    assert!(s.send(b"\n").is_ok());
    assert!(pump_until(&mut s, |s| grid_text(s).contains("12 50")));
    let sizes: Vec<(usize, usize)> = s
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.size()),
            Block::Rich(_) => None,
        })
        .collect();
    assert_eq!(
        sizes,
        [(50, 12)],
        "the block the output lands in has the new size"
    );
}

#[test]
fn typing_reaches_the_program() {
    let mut s = start(Box::new(Passthrough), 40, 5, "read x; echo got:$x");
    assert!(s.send(b"abc\n").is_ok());
    assert!(pump_until(&mut s, |s| grid_text(s).contains("got:abc")));
    assert!(pump_until(&mut s, |_| true));
}
