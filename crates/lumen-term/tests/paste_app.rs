//! Pasting into a real program through the app: whether the text arrives
//! bracketed depends on what the program has switched on.

#![cfg(unix)]

use lumen_term::app::PasteOutcome;
use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

fn grid_text(app: &App) -> String {
    app.session()
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
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

/// A program that sets `mode` (`h` or `l`) for bracketed paste, then prints
/// `ready`, reads exactly `count` bytes raw and prints them as hex.
fn reader(mode: &str, count: usize) -> String {
    format!(
        "stty raw -echo; printf '\\033[?2004{mode}'; echo ready; set -m; \
         dd bs=1 count={count} 2>/dev/null | od -An -v -tx1 | tr -d ' \\n'; echo; echo done"
    )
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn ready(app: &mut App) {
    assert!(
        pump_until(app, |a| grid_text(a).contains("ready")),
        "the program is ready"
    );
}

fn received(app: &mut App, expect: &[u8]) {
    let want = hex(expect);
    assert!(
        pump_until(app, |a| grid_text(a).contains("done")),
        "the program read the paste; screen: {}",
        grid_text(app)
    );
    let screen = grid_text(app).replace(['\n', ' '], "");
    assert!(screen.contains(&want), "got {screen}, wanted {want}");
}

#[test]
fn a_bracketed_paste_with_line_breaks_arrives_between_the_markers() {
    let text = "ls\nrm x\n";
    let sent = [b"\x1b[200~".as_slice(), text.as_bytes(), b"\x1b[201~"].concat();
    let mut app = start(&reader("h", sent.len()));
    ready(&mut app);
    assert!(
        app.session().key_modes().bracketed_paste,
        "the app saw DECSET 2004"
    );
    assert_eq!(app.paste(text).ok(), Some(PasteOutcome::Sent));
    received(&mut app, &sent);
}

#[test]
fn a_pasted_end_marker_cannot_close_the_bracket_early() {
    let sent = [b"\x1b[200~".as_slice(), b"ab", b"\x1b[201~"].concat();
    let mut app = start(&reader("h", sent.len()));
    ready(&mut app);
    assert_eq!(app.paste("a\x1b[201~b").ok(), Some(PasteOutcome::Sent));
    received(&mut app, &sent);
}

#[test]
fn without_bracketed_paste_a_single_line_is_sent_as_typed() {
    let mut app = start(&reader("l", 5));
    ready(&mut app);
    assert!(!app.session().key_modes().bracketed_paste);
    assert_eq!(app.paste("hello").ok(), Some(PasteOutcome::Sent));
    received(&mut app, b"hello");
}

#[test]
fn without_bracketed_paste_a_multi_line_paste_is_held_and_sends_nothing_until_chosen() {
    let mut app = start(&reader("l", 7));
    ready(&mut app);
    let Some(PasteOutcome::Held(hold)) = app.paste("ls\nrm x").ok() else {
        unreachable!("a multi-line paste is held");
    };
    assert!(hold.preview.contains("U+000A"), "the line break is shown");
    // Nothing reached the program: it still waits for its seven bytes.
    for _ in 0..10 {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !grid_text(&app).contains("done"),
        "nothing was sent while held"
    );
    app.paste_chosen(&hold.single_line)
        .unwrap_or_else(|e| unreachable!("sent: {e}"));
    received(&mut app, b"ls rm x");
}

#[test]
fn turning_bracketed_paste_off_again_stops_the_markers() {
    let script = "stty raw -echo; printf '\\033[?2004h'; printf '\\033[?2004l'; echo ready; set -m; \
                  dd bs=1 count=2 2>/dev/null | od -An -v -tx1 | tr -d ' \\n'; echo; echo done";
    let mut app = start(script);
    ready(&mut app);
    assert!(!app.session().key_modes().bracketed_paste);
    assert_eq!(app.paste("hi").ok(), Some(PasteOutcome::Sent));
    received(&mut app, b"hi");
}

#[test]
fn a_paste_with_a_frame_introducer_is_rejected_and_not_one_byte_reaches_the_program() {
    // The program reads two bytes. The refused paste must leave them unread, so
    // the two bytes it gets are the ones typed afterwards.
    let mut app = start(&reader("h", 2));
    ready(&mut app);
    assert!(matches!(
        app.paste("x\x1b_Lumen frame").ok(),
        Some(PasteOutcome::Rejected(_))
    ));
    app.text("ok")
        .unwrap_or_else(|e| unreachable!("typed: {e}"));
    received(&mut app, b"ok");
}

#[test]
fn a_large_bracketed_paste_arrives_whole() {
    let text = "line of pasted text\n".repeat(3000);
    let total = text.len() + 12;
    let script = format!(
        "stty raw -echo; printf '\\033[?2004h'; echo ready; set -m; \
         head -c {total} | wc -c | tr -d ' \\n'; echo; echo done"
    );
    let mut app = start(&script);
    ready(&mut app);
    // The program reads while the app writes; send in the window's own way.
    let outcome = app.paste(&text);
    assert!(matches!(outcome, Ok(PasteOutcome::Sent)), "{outcome:?}");
    assert!(
        pump_until(&mut app, |a| grid_text(a).contains(&total.to_string())),
        "all {total} bytes arrived: {}",
        grid_text(&app)
    );
}
