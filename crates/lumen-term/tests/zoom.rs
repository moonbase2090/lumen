//! Zooming the text: Command or Control with plus, minus and zero re-lays the
//! window out live, grid and rich content both, and what it draws is what a
//! window started at the new size draws.

#![cfg(unix)]

use lumen_term::actions::Action;
use lumen_term::app::Outcome;
use lumen_term::clipboard::Clipboard;
use lumen_term::{App, Layer};
use std::time::{Duration, Instant};

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn text(&self) -> Option<String> {
        None
    }
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

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&lumen_detector::TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// `printf` text that writes `bytes` exactly.
fn printf_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for b in bytes {
        write!(s, "\\{b:03o}").unwrap_or_default();
    }
    s
}

fn b_layer() -> Box<dyn Layer + Send> {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut l);
    Box::new(l)
}

/// A window of 640 by 480 pixels at `scale`, running `script` behind the
/// sealed-layer arm, once `marker` is on the screen.
fn start(scale: u32, script: &str, marker: &str) -> App {
    let mut app = App::start(b_layer(), "/bin/sh", &["-c", script], scale, 640, 480)
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(
        pump_until(&mut app, |a| a.visible_text().iter().any(|l| l == marker)),
        "the program's output arrives"
    );
    app
}

fn rich_and_grid_script() -> String {
    let mut bytes = frame(
        0x80,
        &string(
            "a paragraph of text long enough to wrap onto several lines once the cells are large, so that the zoom has something to flow again",
        ),
    );
    let mut head = vec![1u8];
    head.extend(string("A heading"));
    bytes.extend(frame(0x81, &head));
    let mut code = string("rust");
    code.extend(string("let zoomed = true;"));
    bytes.extend(frame(0x82, &code));
    format!(
        "printf '{}'; printf 'grid line\\n'; sleep 60",
        printf_of(&bytes)
    )
}

#[test]
fn zoom_in_out_and_reset_re_lay_the_window_out_as_a_window_started_at_that_size_does() {
    let script = rich_and_grid_script();
    let none = NoClipboard;
    let mut app = start(1, &script, "grid line");
    let at = |scale| {
        let a = start(scale, &script, "grid line");
        a.view().to_png().unwrap_or_default()
    };
    let rich = |a: &App| {
        a.session()
            .document()
            .blocks()
            .iter()
            .filter(|b| matches!(b, lumen_term::Block::Rich(_)))
            .count()
    };
    assert_eq!(
        rich(&app),
        3,
        "the paragraph, heading and code are in the flow"
    );
    // The window is as it was started, and the three sizes differ from each other.
    let one = app.view().to_png().unwrap_or_default();
    assert_eq!(one, at(1));
    assert_eq!(app.perform(Action::ZoomIn, &none), Outcome::Redraw);
    assert_eq!(app.scale(), 2);
    assert!(pump_until(&mut app, |a| a
        .visible_text()
        .iter()
        .any(|l| l == "grid line")));
    let two = app.view().to_png().unwrap_or_default();
    assert_ne!(one, two);
    assert_eq!(two, at(2), "zoomed in is what a window started at 2 draws");
    assert_eq!(app.perform(Action::ZoomIn, &none), Outcome::Redraw);
    assert_eq!(app.view().to_png().unwrap_or_default(), at(3));
    // Reset goes back to the size the window started at, and out is one step.
    assert_eq!(app.perform(Action::ZoomReset, &none), Outcome::Redraw);
    assert_eq!(app.scale(), 1);
    assert_eq!(app.view().to_png().unwrap_or_default(), one);
    assert_eq!(app.perform(Action::ZoomOut, &none), Outcome::Nothing);
}

#[test]
fn zoom_re_wraps_the_grid_and_holds_the_place_in_the_output() {
    let none = NoClipboard;
    let count = "i=1; while [ $i -le 300 ]; do echo line$i; i=$((i+1)); done;";
    // One long line, then 300 numbered ones.
    let long = "abcdefghij".repeat(5);
    let script = format!("echo {long}; {count} sleep 60");
    let mut app = start(1, &script, "line300");
    assert!(app.scroll_end(true), "scrolled to the oldest output");
    let top_text = |a: &App| a.visible_text().into_iter().next().unwrap_or_default();
    assert_eq!(top_text(&app), long);
    // Zoomed in, the cells are larger, there are fewer columns, and the long
    // line is cut across rows without losing a character.
    for _ in 0..3 {
        assert_eq!(app.perform(Action::ZoomIn, &none), Outcome::Redraw);
    }
    assert_eq!(app.scale(), 4);
    assert!(pump_until(&mut app, |a| a.visible_text().len() > 1));
    let shown = app.visible_text();
    let cols = shown[0].chars().count();
    assert!(cols < long.len(), "{cols} columns now");
    assert_eq!(shown[0], long[..cols]);
    assert_eq!(shown[1], long[cols..(2 * cols).min(long.len())]);
    // Zoomed back, it is one line again.
    assert_eq!(app.perform(Action::ZoomReset, &none), Outcome::Redraw);
    assert_eq!(top_text(&app), long);
}

#[test]
fn a_zoom_keeps_the_output_the_user_scrolled_to_in_view() {
    let none = NoClipboard;
    let count = "i=1; while [ $i -le 300 ]; do echo line$i; i=$((i+1)); done;";
    let mut app = start(1, &format!("{count} sleep 60"), "line300");
    assert!(app.scroll_end(true));
    assert!(app.scroll_rows(-120), "held 120 rows down from the oldest");
    let first = |a: &App| {
        a.visible_text()
            .into_iter()
            .find(|l| l.starts_with("line"))
            .unwrap_or_default()
    };
    let before = first(&app);
    assert!(!app.following());
    for action in [
        Action::ZoomIn,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
    ] {
        assert_eq!(app.perform(action, &none), Outcome::Redraw);
        assert!(!app.following(), "still holding its place");
        let n = |l: &str| l.trim_start_matches("line").parse::<i64>().unwrap_or(0);
        let shown = app.visible_text();
        let (lo, hi) = (n(&before) - 3, n(&before) + 3);
        assert!(
            shown
                .iter()
                .any(|l| (lo..=hi).contains(&n(l)) && l.starts_with("line")),
            "{before} is near the top after {action:?}, saw {:?}",
            &shown[..shown.len().min(3)]
        );
    }
}

#[test]
fn an_image_keeps_its_pixels_at_every_zoom_and_is_held_to_the_width() {
    // The window keeps its pixel width when the text is zoomed, so a small
    // image is drawn at its own size at every scale, never scaled up and
    // blurred, and a wide one is reduced to the width with its proportions.
    let small = lumen_term::images::Pixels::new(40, 30, Vec::new());
    let wide = lumen_term::images::Pixels::new(1200, 600, Vec::new());
    for scale in 1..=4 {
        let r = lumen_term::Renderer::light(scale)
            .unwrap_or_else(|e| unreachable!("the face loads: {e}"));
        let (cell_w, _) = r.cell();
        let columns = usize::try_from(640 / cell_w).unwrap_or(1);
        assert_eq!(r.image_size(&small, columns), (40, 30), "scale {scale}");
        let (w, h) = r.image_size(&wide, columns);
        assert!(
            w <= 640 - 2 * cell_w && w > 640 - 4 * cell_w,
            "scale {scale}: {w} wide"
        );
        assert!(w.abs_diff(2 * h) <= 1, "scale {scale}: {w} by {h}");
    }
}
