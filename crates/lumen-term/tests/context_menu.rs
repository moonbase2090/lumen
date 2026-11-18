//! The right-click menu: what it offers, when an item is turned off, and when
//! a right click belongs to the terminal instead of the program.

#![cfg(all(unix, any(target_os = "macos", windows)))]

use lumen_term::actions::Action;
use lumen_term::menu::{ContextState, action_of, context_items, id_of};
use lumen_term::mouse::Held;
use lumen_term::{App, Passthrough, Renderer};
use std::time::{Duration, Instant};

fn offered(state: ContextState) -> Vec<(Action, bool)> {
    context_items(state)
        .into_iter()
        .flatten()
        .map(|(action, _, on)| (action, on))
        .collect()
}

#[test]
fn the_menu_offers_copy_paste_select_all_and_zoom_and_turns_off_what_would_do_nothing() {
    let all = ContextState {
        can_copy: true,
        can_paste: true,
    };
    assert_eq!(
        offered(all),
        vec![
            (Action::Copy, true),
            (Action::Paste, true),
            (Action::SelectAll, true),
            (Action::ZoomIn, true),
            (Action::ZoomOut, true),
            (Action::ZoomReset, true),
        ]
    );
    let nothing = ContextState {
        can_copy: false,
        can_paste: false,
    };
    let off: Vec<Action> = offered(nothing)
        .into_iter()
        .filter(|(_, on)| !on)
        .map(|(a, _)| a)
        .collect();
    assert_eq!(off, vec![Action::Copy, Action::Paste]);
    // Select all and zoom never turn off.
    assert!(offered(nothing).contains(&(Action::SelectAll, true)));
}

#[test]
fn each_item_comes_back_as_the_action_it_stands_for() {
    for (action, _, _) in context_items(ContextState {
        can_copy: true,
        can_paste: true,
    })
    .into_iter()
    .flatten()
    {
        assert_eq!(action_of(id_of(action)), Some(action));
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

fn ready(app: &mut App, row: &str) {
    assert!(pump_until(app, |a| {
        a.session().document().blocks().iter().any(|b| match b {
            lumen_term::Block::Grid(g) => g.text_rows().iter().any(|r| r == row),
            lumen_term::Block::Rich(_) => false,
        })
    }));
}

fn at(col: u32, row: u32) -> (u32, u32) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("renderer: {e}"));
    let (cw, ch) = r.cell();
    (
        lumen_term::GUTTER_COLUMNS * cw + r.gutter_pad() + col * cw + 1,
        lumen_term::HEADER_ROWS * ch + row * ch + ch / 2,
    )
}

#[test]
fn a_right_click_opens_the_menu_unless_the_program_asked_for_the_mouse_and_shift_takes_it_back() {
    let mut plain = start("echo plain row; sleep 30");
    ready(&mut plain, "plain row");
    let (x, y) = at(2, 0);
    assert!(plain.owns_context_click(x, y, Held::default()));

    let mut mouse = start("printf '\\033[?1000h\\033[?1006h'; echo mouse mode on; sleep 30");
    ready(&mut mouse, "mouse mode on");
    let (x, y) = at(2, 0);
    assert!(
        !mouse.owns_context_click(x, y, Held::default()),
        "the program asked for the mouse, so it gets the click"
    );
    let shift = Held {
        shift: true,
        ..Held::default()
    };
    assert!(mouse.owns_context_click(x, y, shift));
}
