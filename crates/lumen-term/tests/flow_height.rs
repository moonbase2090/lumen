//! A grid block takes only the rows it has used in the window's flow while
//! nothing has scrolled into history, so the rows a program has not reached yet
//! do not push the output before them out of view. Once output has scrolled,
//! or on the alternate screen, the whole screen stays: a cleared screen shows
//! blank. A real program on a real PTY, behind the arm that draws rich blocks.

#![cfg(unix)]

use lumen_term::{App, Layer, Renderer};
use std::time::{Duration, Instant};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

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

/// A paragraph and a heading, as Track 0 frames.
fn rich_frames() -> Vec<u8> {
    let mut bytes = frame(0x80, &string("A paragraph a program wrote."));
    let mut head = vec![1u8];
    head.extend(string("A heading"));
    bytes.extend(frame(0x81, &head));
    bytes
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

/// Wait for `done`, which turns true on the program's output: after each check
/// the test blocks until the program writes again. The bound only ends a hung
/// test; there is no polling interval.
fn until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if done(app) {
            return true;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        app.pump_wait(left);
    }
}

/// A window running `script`, once a grid row reads `marker`. Each marker is
/// written in two parts in the script, so only the output shows it.
fn start(script: &str, marker: &str) -> App {
    let mut app = App::start(b_layer(), "/bin/sh", &["-c", script], 1, WIDTH, HEIGHT)
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(
        until(&mut app, |a| a.visible_text().iter().any(|l| l == marker)),
        "the program's output arrives: {:?}",
        app.visible_text()
    );
    app
}

fn cell_height() -> u32 {
    Renderer::light(1)
        .unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
        .cell()
        .1
}

#[test]
fn rich_blocks_written_before_the_prompt_stay_in_view() {
    let script = format!(
        "printf '{}'; printf 'pro''mpt-row\\n'; sleep 60",
        printf_of(&rich_frames())
    );
    let mut app = start(&script, "prompt-row");
    assert!(app.following(), "the view follows the newest output");
    // Everything fits: the rich blocks and the row after them are in the view,
    // so there is nothing above it to go to.
    assert!(
        !app.scroll_end(true),
        "nothing above the view: the rich blocks are in it"
    );
}

#[test]
fn a_rich_block_after_grid_output_starts_right_after_its_last_row() {
    let script = format!(
        "printf 'first-row\\nsecond-row\\n'; printf '{}'; printf 'af''ter\\n'; sleep 60",
        printf_of(&rich_frames())
    );
    let app = start(&script, "after");
    let renderer = Renderer::light(1).unwrap_or_else(|e| unreachable!("face: {e}"));
    let doc = app.session().document();
    let flow = renderer.flow(doc, doc.columns());
    let first_grid_lines = flow.iter().filter(|l| l.block == 0).count();
    let Some(rich_top) = flow.iter().find(|l| l.block == 1).map(|l| l.top) else {
        unreachable!("the rich block is in the flow: {flow:?}")
    };
    assert_eq!(
        first_grid_lines, 2,
        "the grid block before it is its two rows"
    );
    // Its first line of text sits under its label, one row into the block.
    // Before, the block began a whole screen (here 25 rows) down.
    assert!(
        rich_top <= 4 * cell_height(),
        "the rich block follows the two rows, not a screen of blank ones: top {rich_top}"
    );
}

#[test]
fn a_clear_after_output_has_scrolled_still_shows_a_blank_screen() {
    let mut app = start(
        "i=0; while [ $i -lt 200 ]; do echo line-$i; i=$((i+1)); done; \
         printf '\\033[H\\033[2J'; printf 'fre''sh\\n'; sleep 60",
        "fresh",
    );
    assert!(until(&mut app, App::following));
    let shown = app.visible_text();
    assert_eq!(
        shown.iter().find(|l| !l.is_empty()).map(String::as_str),
        Some("fresh"),
        "the cleared screen is what is in view, not the history above it: {shown:?}"
    );
    assert!(!shown.iter().any(|l| l.starts_with("line-")), "{shown:?}");
}

#[test]
fn a_program_that_draws_on_the_bottom_row_keeps_the_whole_screen() {
    // The cursor goes back to the top after the bottom row is drawn.
    let app = start(
        "printf '\\033[999;1Hbottom''-row\\033[H'; printf 'to''p-row'; sleep 60",
        "top-row",
    );
    let shown = app.visible_text();
    assert!(
        shown.iter().any(|l| l == "bottom-row"),
        "the bottom row is in the flow and in view: {shown:?}"
    );
}
