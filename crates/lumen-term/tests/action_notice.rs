//! A window action that does not happen, such as opening the settings file
//! without a home directory, is not silent: the window shows the terminal's
//! own notice, ahead of anything a program asked for, and a press on it
//! dismisses only that notice.

#![cfg(all(unix, feature = "window"))]

use lumen_term::app::{PANE_LABEL, cells_for, window_for};
use lumen_term::notify::{Place, Settings};
use lumen_term::{App, Block, Passthrough, Renderer, layout, notification_area};
use std::time::{Duration, Instant};

const W: u32 = 800;
const H: u32 = 600;

/// Takes OS notifications and drops them: this test is about toasts.
struct Drop;

impl lumen_term::notify::OsSink for Drop {
    fn send(&mut self, _: &lumen_term::notify::OsNotification) {}
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

/// Wait for `done`, which turns true on the program's output: after each check
/// the test blocks until the program writes again. The bound only ends a hung
/// test; there is no polling interval.
fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
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

#[test]
fn the_notice_is_drawn_first_and_a_press_dismisses_only_it() {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &[
            "-c",
            "printf '\\033]9;from-the-program\\007'; echo READY; exec sleep 60",
        ],
        1,
        W,
        H,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    app.configure_notifications(
        Settings {
            allow_unsigned: true,
            ..Settings::default()
        },
        Place::Local,
        Box::new(Drop),
    );
    assert!(pump_until(&mut app, |a| grid_text(a).contains("READY")
        && a.notifier().shown().len() == 1));

    let why = "Settings not opened: there is no home directory (HOME is not set)";
    app.show_notice(why);
    let texts = |a: &App| -> Vec<String> { a.toasts().iter().map(|t| t.text.clone()).collect() };
    let shown = texts(&app);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert_eq!(shown[0], why, "the terminal's notice comes first");
    assert_eq!(app.toasts()[0].layer, 0, "and it is the terminal's own");
    assert!(shown[1].contains(PANE_LABEL), "{shown:?}");

    // A press on the notice dismisses it and leaves the program's toast.
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"));
    let (columns, rows) = cells_for(W, H, r.cell());
    let win = window_for(columns, rows);
    let note = notification_area(&layout(
        win.cols,
        win.rows,
        win.editor_row,
        win.editor_rows,
        None,
    ));
    let (cw, ch) = r.cell();
    let hit = (note.y * ch..(note.y + note.h) * ch)
        .flat_map(|y| (note.x * cw..(note.x + note.w) * cw).map(move |x| (x, y)))
        .find(|(x, y)| app.click_would_hit(*x, *y) == Some(0));
    let Some((x, y)) = hit else {
        unreachable!("the notice is drawn")
    };
    assert!(app.click(x, y));
    let left = texts(&app);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].contains(PANE_LABEL), "{left:?}");
}
