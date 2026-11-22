//! Clicking at every zoom: what a press lands on at 200 percent and up is what
//! it lands on at the display's own size. A real program runs behind a real
//! PTY and the window is zoomed with the same action a chord asks for; the
//! system opener is replaced by one that records what it was given.

#![cfg(all(unix, feature = "links-clipboard"))]

use lumen_term::actions::{Action, MAX_SCALE};
use lumen_term::clipboard::Clipboard;
use lumen_term::linkprompt::LinkOpener;
use lumen_term::{App, Passthrough};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (800, 480);
const URL: &str = "https://a.io/x";
const SAY: &str = "echo https://a.io/x; sleep 60";

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn text(&self) -> Option<String> {
        None
    }
}

#[derive(Clone, Default)]
struct Opened(Arc<Mutex<Vec<String>>>);

impl LinkOpener for Opened {
    fn open(&mut self, target: &str) -> std::io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(target.to_owned());
        Ok(())
    }
}

impl Opened {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A window at scale 1 whose program has said its line.
fn start() -> (App, Opened) {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", SAY],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let opened = Opened::default();
    app.use_link_opener(Box::new(opened.clone()));
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        if app.visible_text().iter().any(|r| r == URL) {
            return (app, opened);
        }
        let left = end.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "the program's output arrives");
        app.pump_wait(left);
    }
}

/// Zoom in to `scale` the way the chord does, and wait for the program's line
/// to be on the screen again after it re-wraps.
fn zoom_to(app: &mut App, scale: u32) {
    while app.scale() < scale {
        app.perform(Action::ZoomIn, &NoClipboard);
    }
    assert_eq!(app.scale(), scale);
}

/// The pixels of the window over the live cell `(col, row)`.
fn pixels_of_cell(app: &App, col: u32, row: u32) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    for y in (0..SIZE.1).step_by(2) {
        for x in (0..SIZE.0).step_by(2) {
            if app.live_cell(x, y) == Some((col, row)) {
                found.push((x, y));
            }
        }
    }
    found
}

fn row_with_the_link(app: &App) -> (u32, u32) {
    let rows = app.visible_text();
    let row = rows
        .iter()
        .position(|r| r.contains(URL))
        .unwrap_or_else(|| unreachable!("the link is on screen: {rows:?}"));
    (
        u32::try_from(rows[row].find(URL).unwrap_or(0)).unwrap_or(0),
        u32::try_from(row).unwrap_or(0),
    )
}

#[test]
fn a_press_on_a_link_asks_and_the_buttons_answer_at_every_zoom() {
    for scale in 1..=MAX_SCALE {
        let (mut app, opened) = start();
        zoom_to(&mut app, scale);
        let (col, row) = row_with_the_link(&app);
        // Every pixel of a cell in the middle of the address is that cell.
        let pixels = pixels_of_cell(&app, col + 6, row);
        assert!(!pixels.is_empty(), "scale {scale}: the cell has pixels");
        let (cw, ch) = (app.cell_height() / 2, app.cell_height());
        assert!(
            u32::try_from(pixels.len()).unwrap_or(u32::MAX) >= cw * ch / 8,
            "scale {scale}: the cell covers about a cell's pixels"
        );
        for (x, y) in [
            pixels[0],
            pixels[pixels.len() / 2],
            pixels[pixels.len() - 1],
        ] {
            assert_eq!(
                app.link_at(x, y).map(|h| h.target),
                Some(URL.to_owned()),
                "scale {scale}: ({x}, {y})"
            );
        }
        let (x, y) = pixels[pixels.len() / 2];
        assert!(
            app.link_press(x, y),
            "scale {scale}: the press is the terminal's"
        );
        let boxed = app
            .link_prompt_box()
            .unwrap_or_else(|| unreachable!("scale {scale}: the question is up"));
        let inside = |(bx, by, bw, bh): (u32, u32, u32, u32),
                      (ox, oy, ow, oh): (u32, u32, u32, u32)| {
            bx >= ox && by >= oy && bx + bw <= ox + ow && by + bh <= oy + oh
        };
        assert!(
            inside(boxed.rect, (0, 0, SIZE.0, SIZE.1)),
            "scale {scale}: the question fits the window: {:?}",
            boxed.rect
        );
        assert_eq!(boxed.buttons.len(), 2, "scale {scale}: two buttons");
        for (label, rect) in &boxed.buttons {
            assert!(
                inside(*rect, boxed.rect),
                "scale {scale}: {label} is in the box"
            );
        }
        assert!(
            opened.all().is_empty(),
            "scale {scale}: nothing opens before the answer"
        );
        let open = boxed
            .buttons
            .iter()
            .find(|(l, _)| l.contains("Open"))
            .unwrap_or_else(|| unreachable!("an Open button"))
            .1;
        assert!(app.prompt_click(open.0 + open.2 / 2, open.1 + open.3 / 2));
        assert_eq!(
            opened.all(),
            vec![URL],
            "scale {scale}: Open opens the address"
        );
        assert!(app.link_prompt().is_none());
    }
}

#[test]
fn the_right_click_menu_stays_in_the_window_and_its_last_entry_answers_at_every_zoom() {
    for scale in 1..=MAX_SCALE {
        let (mut app, _) = start();
        zoom_to(&mut app, scale);
        assert!(
            app.open_context_menu(120, 90, true),
            "scale {scale}: it opens"
        );
        let placed = app
            .context_menu_box()
            .unwrap_or_else(|| unreachable!("scale {scale}: open"));
        let (x, y, w, h) = placed.rect;
        assert!(
            x + w <= SIZE.0 && y + h <= SIZE.1,
            "scale {scale}: the menu fits the window: {:?}",
            placed.rect
        );
        for (i, (rx, ry, rw, rh)) in placed.rows.iter().enumerate() {
            assert!(
                *rx >= x && *ry >= y && rx + rw <= x + w && ry + rh <= y + h,
                "scale {scale}: entry {i} is inside the menu"
            );
        }
        // The last entry, the one furthest down, is under the pointer too.
        let last = placed.rows.len() - 1;
        let (rx, ry, rw, rh) = placed.rows[last];
        assert!(app.menu_click(rx + rw / 2, ry + rh / 2));
        assert!(
            app.take_menu_choice().is_some(),
            "scale {scale}: a press on the last entry chooses it"
        );
    }
}
