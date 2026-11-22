//! A selection redraws the text on the palette's selection colour: the
//! background becomes the selection colour, and the glyphs, whatever colour
//! the program gave them, are drawn in the selection's text colour.

#![cfg(unix)]

use lumen_term::actions::Action;
use lumen_term::clipboard::Clipboard;
use lumen_term::design::theme;
use lumen_term::{App, Passthrough};
use std::time::{Duration, Instant};

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn text(&self) -> Option<String> {
        None
    }
}

fn start() -> App {
    start_with("\\033[33mYELLOW\\033[0m \\033[44mblue bg\\033[0m done", 2)
}

fn start_with(line: &str, scale: u32) -> App {
    // Text in a colour of its own (ANSI yellow) and on a background of its own.
    let script = format!("printf '{}\\n'; sleep 60", line.replace('%', "%%"));
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        scale,
        1600,
        900,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let end = Instant::now() + Duration::from_secs(20);
    let marker = line.rsplit(' ').next().unwrap_or("done");
    while !app.visible_text().iter().any(|r| r.contains(marker)) {
        app.pump();
        assert!(Instant::now() < end, "the program's output arrives");
        std::thread::sleep(Duration::from_millis(20));
    }
    app
}

fn count(app: &App, colour: [u8; 3]) -> usize {
    let frame = app.view();
    let mut n = 0;
    for y in 0..frame.height {
        for x in 0..frame.width {
            n += usize::from(frame.pixel(x, y) == Some(colour));
        }
    }
    n
}

#[test]
fn selected_text_is_redrawn_on_the_selection_colour_in_the_selection_text_colour() {
    let t = theme();
    let yellow = t.ansi[3];
    let mut app = start();
    assert!(count(&app, yellow) > 0, "unselected, the text is yellow");
    assert_eq!(count(&app, t.selection), 0);
    assert_eq!(
        app.perform(Action::SelectAll, &NoClipboard),
        lumen_term::app::Outcome::Redraw
    );
    assert!(
        count(&app, t.selection) > 100,
        "the background is the selection colour"
    );
    assert!(
        count(&app, t.selection_text) > 0,
        "the glyphs are in the selection text colour"
    );
    assert_eq!(
        count(&app, yellow),
        0,
        "no pixel keeps the colour the program gave it"
    );
    assert_eq!(
        count(&app, t.ansi[4]),
        0,
        "nor the background the program gave"
    );
}

#[test]
fn every_selected_glyph_is_on_the_selection_colour_at_every_zoom() {
    // Plain antialiased glyphs of many shades, in a colour of the program's own.
    let line = "\\033[33mYELLOW@%#QWmwg89\\033[0m";
    for scale in 1..=4 {
        let t = theme();
        let yellow = t.ansi[3];
        let mut app = start_with(&format!("{line} done"), scale);
        let before = app.view();
        let (mut top, mut bottom, mut first, mut last) = (u32::MAX, 0, u32::MAX, 0);
        for y in 0..before.height {
            for x in 0..before.width {
                if before.pixel(x, y) == Some(yellow) {
                    top = top.min(y);
                    bottom = bottom.max(y);
                    first = first.min(x);
                    last = last.max(x);
                }
            }
        }
        assert!(top <= bottom, "the line is on screen at {scale}x");
        assert_eq!(
            app.perform(Action::SelectAll, &NoClipboard),
            lumen_term::app::Outcome::Redraw
        );
        let after = app.view();
        // Every pixel of the text, edges included, lies between the selection
        // colour and the selection's text colour: none is left on the program's
        // colours or on the old tint.
        for x in first..=last {
            for y in top..=bottom {
                let p = after.pixel(x, y).unwrap_or([0; 3]);
                let between = (0..3).all(|k| {
                    let (lo, hi) = (
                        t.selection[k].min(t.selection_text[k]),
                        t.selection[k].max(t.selection_text[k]),
                    );
                    (lo.saturating_sub(1)..=hi.saturating_add(1)).contains(&p[k])
                });
                assert!(
                    between,
                    "pixel {x},{y} at {scale}x is {p:?}, off the selection pair"
                );
            }
        }
        let width = (last - first + 1) / 4;
        for part in 0..4 {
            let from = first + part * width;
            let ink = (from..from + width)
                .any(|x| (top..=bottom).any(|y| after.pixel(x, y) == Some(t.selection_text)));
            assert!(
                ink,
                "part {part} of the text at {scale}x has the selection text colour"
            );
        }
        for y in top..=bottom {
            for x in first..=last {
                assert_ne!(
                    after.pixel(x, y),
                    Some(yellow),
                    "no ANSI yellow at {scale}x"
                );
            }
        }
    }
}

#[test]
fn block_glyphs_keep_their_fill_when_selected_at_every_zoom() {
    // A full block and a three-quarter block are mostly ink: the fill must stay
    // the selection's text colour and the gap the selection colour.
    let line = "\\033[33m█▊\\033[0m done";
    for scale in 1..=4 {
        let t = theme();
        let yellow = t.ansi[3];
        let mut app = start_with(line, scale);
        let before = app.view();
        let (cell_w, _) = lumen_term::Renderer::light(scale)
            .unwrap_or_else(|e| unreachable!("renderer: {e}"))
            .cell();
        let (mut top, mut bottom, mut first) = (u32::MAX, 0, u32::MAX);
        for y in 0..before.height {
            for x in 0..before.width {
                if before.pixel(x, y) == Some(yellow) {
                    top = top.min(y);
                    bottom = bottom.max(y);
                    first = first.min(x);
                }
            }
        }
        assert!(top <= bottom, "the blocks are on screen at {scale}x");
        let mid = u32::midpoint(top, bottom);
        assert_eq!(
            app.perform(Action::SelectAll, &NoClipboard),
            lumen_term::app::Outcome::Redraw
        );
        let after = app.view();
        // The full block's left edge is the cell's: its middle is all ink.
        assert_eq!(
            after.pixel(first + cell_w / 2, mid),
            Some(t.selection_text),
            "the full block is still the text colour at {scale}x"
        );
        // The second cell's left part is ink and its right edge is empty.
        assert_eq!(
            after.pixel(first + cell_w + 1, mid),
            Some(t.selection_text),
            "the filled part of the three-quarter block is the text colour at {scale}x"
        );
        assert_eq!(
            after.pixel(first + 2 * cell_w - 1, mid),
            Some(t.selection),
            "the empty part of the three-quarter block is the selection colour at {scale}x"
        );
    }
}
