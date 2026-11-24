//! The primary selection on Linux: a finished mouse selection is offered to it
//! whatever the copy setting, and a middle click is the terminal's to paste it
//! unless the program asked for mouse reports (Shift takes it back). A real
//! program on a real PTY, through the calls the window makes.

#![cfg(unix)]

use lumen_term::actions::Action;
use lumen_term::app::Outcome;
use lumen_term::clipboard::Clipboard;
use lumen_term::mouse::{Button, Held};
use lumen_term::{App, Passthrough, Renderer};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// Stands in for the primary selection: keeps what it is given.
struct Primary(RefCell<Option<String>>);

impl Clipboard for Primary {
    fn text(&self) -> Option<String> {
        self.0.borrow().clone()
    }
    fn set_text(&self, text: &str) -> bool {
        *self.0.borrow_mut() = Some(text.to_owned());
        true
    }
}

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

/// Wait for the program to print `row`. Each wait ends when the program
/// writes; the limit only ends a test that never gets its signal.
fn ready(app: &mut App, row: &str) {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let seen = app.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == row),
            lumen_term::Block::Rich(_) => false,
        });
        if seen {
            return;
        }
        let left = end.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "the program did not print {row:?}");
        app.pump_wait(left);
    }
}

/// The pixel at the left edge of column `col` and the middle of row `row` of
/// the content.
fn at(col: u32, row: u32) -> (u32, u32) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let (cw, ch) = r.cell();
    (
        // The empty gutter is drawn two cells wide, then the padding.
        2 * cw + r.gutter_pad() + col * cw + 1,
        lumen_term::HEADER_ROWS * ch + row * ch + ch / 2,
    )
}

#[test]
fn a_finished_drag_is_offered_to_the_primary_selection_whatever_the_copy_setting() {
    let mut app = start("echo hello world; echo second line; sleep 30");
    ready(&mut app, "second line");
    app.set_copy_on_select(false);
    let held = Held::default();
    let (x0, y0) = at(0, 0);
    let (x1, _) = at(5, 0);

    // A click with no drag selects nothing, so nothing is offered.
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_release(x0, y0, Button::Left, held).ok();
    assert!(!app.take_finished_selection());

    // A drag is offered once, when the button is released, even though the
    // copy setting is off.
    app.mouse_press(x0, y0, Button::Left, held).ok();
    app.mouse_move(x1, y0, held).ok();
    assert!(!app.take_finished_selection(), "still dragging");
    app.mouse_release(x1, y0, Button::Left, held).ok();
    assert!(!app.take_copy_on_select(), "the copy setting is off");
    assert!(app.take_finished_selection());
    assert!(!app.take_finished_selection(), "offered once");

    // The window puts it there with a copy, so the primary selection gets the
    // text a copy would carry.
    let primary = Primary(RefCell::new(None));
    assert_eq!(app.perform(Action::Copy, &primary), Outcome::Nothing);
    assert_eq!(primary.0.borrow().as_deref(), Some("hello"));

    // Select all is not a mouse selection.
    assert_eq!(app.perform(Action::SelectAll, &primary), Outcome::Redraw);
    assert!(!app.take_finished_selection());
}

#[test]
fn a_middle_click_is_the_terminal_s_unless_the_program_asked_for_the_mouse_and_shift_takes_it_back()
{
    let mut plain = start("echo plain row; sleep 30");
    ready(&mut plain, "plain row");
    let (x, y) = at(2, 0);
    assert!(plain.owns_middle_click(x, y, Held::default()));

    let mut mouse = start("printf '\\033[?1000h\\033[?1006h'; echo mouse mode on; sleep 30");
    ready(&mut mouse, "mouse mode on");
    let (x, y) = at(2, 0);
    assert!(
        !mouse.owns_middle_click(x, y, Held::default()),
        "the program asked for the mouse, so it gets the click"
    );
    let shift = Held {
        shift: true,
        ..Held::default()
    };
    assert!(mouse.owns_middle_click(x, y, shift));
}

#[cfg(feature = "window")]
#[test]
fn the_copies_come_from_the_pane_that_made_the_selection_when_the_focus_moved_during_the_drag() {
    use lumen_term::host::copy_finished_selection;
    use lumen_term::panes::{Panes, Split};

    let mut left = start("echo LEFT; sleep 30");
    ready(&mut left, "LEFT");
    let mut right = start("echo RIGHT; sleep 30");
    ready(&mut right, "RIGHT");
    let held = Held::default();
    let (x0, y0) = at(0, 0);
    let (x1, _) = at(5, 0);

    // The right pane has an earlier selection of its own.
    right.mouse_press(x0, y0, Button::Left, held).ok();
    right.mouse_move(x1, y0, held).ok();
    right.mouse_release(x1, y0, Button::Left, held).ok();
    assert_eq!(right.selected_text().as_deref(), Some("RIGHT"));
    let _ = right.take_copy_on_select();
    let _ = right.take_finished_selection();

    let mut panes = Panes::new(left);
    panes.split(right, Split::Right);
    assert!(panes.focus(0));

    // A drag starts in the left pane, the focus moves to the right one while
    // the button is held, and the release goes to the left pane, which
    // captured the drag.
    let left = panes
        .get_mut(0)
        .unwrap_or_else(|| unreachable!("two panes"));
    left.mouse_press(x0, y0, Button::Left, held).ok();
    left.mouse_move(x1, y0, held).ok();
    assert!(panes.focus_next(), "the focus moves during the drag");
    assert_eq!(panes.focus_index(), 1);
    let left = panes
        .get_mut(0)
        .unwrap_or_else(|| unreachable!("two panes"));
    left.mouse_release(x1, y0, Button::Left, held).ok();

    let clipboard = Primary(RefCell::new(None));
    let primary = Primary(RefCell::new(None));
    copy_finished_selection(&mut panes, 0, &clipboard, Some(&primary));
    assert_eq!(primary.0.borrow().as_deref(), Some("LEFT"));
    assert_eq!(
        clipboard.0.borrow().as_deref(),
        Some("LEFT"),
        "copy on select, on by default, copies the same pane's text"
    );

    // Each finished selection is copied once.
    let again = Primary(RefCell::new(None));
    copy_finished_selection(&mut panes, 0, &again, Some(&again));
    assert_eq!(*again.0.borrow(), None);
}
