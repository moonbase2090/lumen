//! The right-click menu the terminal draws itself, where the system has none:
//! what it paints over a program's output, how a press or a key picks an entry,
//! and that nothing reaches the program while it is open.

#![cfg(unix)]

use lumen_term::actions::Action;
use lumen_term::mouse::{Button, Held};
use lumen_term::{App, Key, Passthrough};
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

fn ready(app: &mut App, row: &str) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        app.pump();
        let seen = app.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == row),
            lumen_term::Block::Rich(_) => false,
        });
        if seen {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    unreachable!("the program did not print {row:?}");
}

fn centre(rect: (u32, u32, u32, u32)) -> (u32, u32) {
    (rect.0 + rect.2 / 2, rect.1 + rect.3 / 2)
}

/// Open the menu at a point inside the window and return the rectangle of the
/// entry at `index` of its items.
fn open(app: &mut App, can_paste: bool, index: usize) -> (u32, u32, u32, u32) {
    assert!(app.open_context_menu(120, 90, can_paste));
    let placed = app
        .context_menu_box()
        .unwrap_or_else(|| unreachable!("open"));
    placed.rows[index]
}

const COPY: usize = 0;
const PASTE: usize = 1;
const SELECT_ALL: usize = 2;
const SEPARATOR: usize = 3;
const ZOOM_IN: usize = 4;

#[test]
fn the_menu_is_painted_over_the_program_s_output() {
    let mut app = start("echo menu under test; sleep 30");
    ready(&mut app, "menu under test");
    let before = app.view();
    let row = open(&mut app, true, SELECT_ALL);
    let after = app.view();
    assert_ne!(
        before.rgba, after.rgba,
        "opening the menu changes the frame"
    );
    let rect = app.context_menu_box().map_or((0, 0, 0, 0), |b| b.rect);
    assert_ne!(
        after.pixel(rect.0, rect.1),
        before.pixel(rect.0, rect.1),
        "the menu's border is drawn over what was there"
    );
    assert!(
        row.0 >= rect.0 && row.0 + row.2 <= rect.0 + rect.2,
        "rows are inside the box"
    );
    assert!(app.close_context_menu());
    assert_eq!(
        app.view().rgba,
        before.rgba,
        "closing it restores the frame"
    );
}

#[test]
fn a_press_on_an_entry_chooses_it_and_the_release_goes_nowhere() {
    let mut app = start("echo pick; sleep 30");
    ready(&mut app, "pick");
    let (x, y) = centre(open(&mut app, true, SELECT_ALL));
    assert!(
        app.mouse_press(x, y, Button::Left, Held::default())
            .unwrap_or(false)
    );
    assert_eq!(app.take_menu_choice(), Some(Action::SelectAll));
    assert_eq!(app.take_menu_choice(), None, "a choice is taken once");
    assert!(app.context_menu().is_none(), "the menu closes on a choice");
    assert!(
        !app.mouse_release(x, y, Button::Left, Held::default())
            .unwrap_or(true)
    );
}

#[test]
fn an_entry_that_is_off_and_the_separator_do_nothing_and_the_menu_stays() {
    let mut app = start("echo off; sleep 30");
    ready(&mut app, "off");
    for index in [COPY, SEPARATOR] {
        let (x, y) = centre(open(&mut app, false, index));
        let _ = app.mouse_press(x, y, Button::Left, Held::default());
        let _ = app.mouse_release(x, y, Button::Left, Held::default());
        assert_eq!(app.take_menu_choice(), None);
        assert!(app.context_menu().is_some(), "the menu stays open");
    }
    let (x, y) = centre(open(&mut app, false, PASTE));
    let _ = app.mouse_press(x, y, Button::Left, Held::default());
    assert_eq!(
        app.take_menu_choice(),
        None,
        "Paste is off with no text to paste"
    );
    let (x, y) = centre(open(&mut app, true, PASTE));
    let _ = app.mouse_press(x, y, Button::Left, Held::default());
    assert_eq!(app.take_menu_choice(), Some(Action::Paste));
}

#[test]
fn a_press_away_from_the_menu_closes_it_and_chooses_nothing() {
    let mut app = start("echo away; sleep 30");
    ready(&mut app, "away");
    let _ = open(&mut app, true, ZOOM_IN);
    assert!(
        app.mouse_press(700, 500, Button::Left, Held::default())
            .unwrap_or(false)
    );
    assert!(app.context_menu().is_none());
    assert_eq!(app.take_menu_choice(), None);
}

#[test]
fn the_keys_move_choose_and_close_and_the_program_hears_none_of_them() {
    let mut app = start("echo keys; sleep 30");
    ready(&mut app, "keys");
    let none = lumen_term::Mods::default();
    let _ = open(&mut app, false, COPY);
    // Copy and Paste are off: Down lands on Select All, then Zoom In.
    app.key(Key::Down, none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        app.context_menu()
            .and_then(lumen_term::contextmenu::ContextMenu::chosen),
        Some(Action::SelectAll)
    );
    app.key(Key::Down, none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    app.key(Key::Up, none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    // A typed character and pasted text are swallowed, and the menu stays.
    app.key(Key::Char('x'), none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    app.text("pasted").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.context_menu().is_some());
    app.key(Key::Enter, none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(app.take_menu_choice(), Some(Action::SelectAll));
    assert!(app.context_menu().is_none());

    let _ = open(&mut app, true, COPY);
    app.key(Key::Escape, none)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.context_menu().is_none());
    assert_eq!(app.take_menu_choice(), None);
}

#[test]
fn the_pointer_highlights_the_entry_under_it_and_only_one_that_is_on() {
    let mut app = start("echo hover; sleep 30");
    ready(&mut app, "hover");
    let on = centre(open(&mut app, false, SELECT_ALL));
    assert!(app.mouse_move(on.0, on.1, Held::default()).unwrap_or(false));
    assert_eq!(
        app.context_menu()
            .and_then(lumen_term::contextmenu::ContextMenu::hover),
        Some(SELECT_ALL)
    );
    let off = centre(
        app.context_menu_box()
            .map_or((0, 0, 0, 0), |b| b.rows[COPY]),
    );
    let _ = app.mouse_move(off.0, off.1, Held::default());
    assert_eq!(
        app.context_menu()
            .and_then(lumen_term::contextmenu::ContextMenu::hover),
        None,
        "Copy is off"
    );
}

#[test]
fn the_menu_stays_inside_the_window_near_an_edge() {
    let mut app = start("echo edge; sleep 30");
    ready(&mut app, "edge");
    assert!(app.open_context_menu(799, 599, true));
    let placed = app
        .context_menu_box()
        .unwrap_or_else(|| unreachable!("open"));
    let (x, y, w, h) = placed.rect;
    assert!(
        x + w <= 800 && y + h <= 600,
        "{:?} inside 800x600",
        placed.rect
    );
}

#[test]
fn copy_is_on_only_with_a_selection() {
    let mut app = start("echo selectable; sleep 30");
    ready(&mut app, "selectable");
    assert!(app.open_context_menu(100, 100, true));
    assert!(!app.context_menu().is_some_and(|m| m.is_on(COPY)));
    app.close_context_menu();
    let _ = app.perform(Action::SelectAll, &lumen_term::clipboard::SystemClipboard);
    assert!(app.has_selection());
    assert!(app.open_context_menu(100, 100, true));
    assert!(app.context_menu().is_some_and(|m| m.is_on(COPY)));
}
