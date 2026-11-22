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
    // Text in a colour of its own (ANSI yellow) and on a background of its own.
    let script = "printf '\\033[33mYELLOW\\033[0m \\033[44mblue bg\\033[0m done\\n'; sleep 60";
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        2,
        800,
        400,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let end = Instant::now() + Duration::from_secs(20);
    while !app.visible_text().iter().any(|r| r.contains("done")) {
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
