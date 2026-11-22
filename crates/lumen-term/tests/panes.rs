//! A tab split into panes: each pane is a session of its own, drawn in its
//! place with dividers between, and the focus says which one the keyboard
//! reaches.

#![cfg(unix)]

use lumen_term::design::theme;
use lumen_term::panes::{Panes, Split};
use lumen_term::{App, Frame, Passthrough};
use std::time::{Duration, Instant};

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 500;
const GAP: u32 = 2;

fn start(marker: &str) -> App {
    let script = format!("printf '{marker}\\n'; sleep 60");
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        WIDTH,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let end = Instant::now() + Duration::from_secs(20);
    while !app.visible_text().iter().any(|r| r.contains(marker)) {
        app.pump();
        assert!(Instant::now() < end, "{marker} arrives");
        std::thread::sleep(Duration::from_millis(20));
    }
    app
}

fn lay_out(panes: &mut Panes<App>) -> lumen_term::panes::Layout {
    let layout = panes.layout(WIDTH, HEIGHT, GAP);
    for (app, r) in panes.all_mut().into_iter().zip(&layout.panes) {
        app.resize(r.w, r.h)
            .unwrap_or_else(|e| unreachable!("resize: {e}"));
    }
    layout
}

fn compose(panes: &Panes<App>, layout: &lumen_term::panes::Layout) -> Frame {
    let views: Vec<Frame> = panes.all().into_iter().map(App::view).collect();
    panes
        .focused()
        .compose_panes((WIDTH, HEIGHT), layout, &views, panes.focus_index())
}

fn has(app: &App, marker: &str) -> bool {
    app.visible_text().iter().any(|r| r.contains(marker))
}

#[test]
fn each_pane_shows_its_own_session_in_its_place_with_a_divider_and_a_focus_mark() {
    let t = theme();
    let mut panes = Panes::new(start("LEFTPANE"));
    panes.split(start("RIGHTPANE"), Split::Right);
    let layout = lay_out(&mut panes);
    let frame = compose(&panes, &layout);
    let (left, right) = (layout.panes[0], layout.panes[1]);
    let line = layout.dividers[0];
    assert_eq!(
        frame.pixel(line.x, line.h / 2),
        Some(t.divider),
        "the divider"
    );
    assert!(left.x + left.w <= line.x && line.x + line.w <= right.x);
    // Each pane's pixels are its own view, drawn from its own corner.
    let views: Vec<Frame> = panes.all().into_iter().map(App::view).collect();
    for (rect, view) in [left, right].into_iter().zip(&views) {
        assert_eq!((view.width, view.height), (rect.w, rect.h));
        for (x, y) in [(40, 60), (rect.w / 2, rect.h / 2), (rect.w - 1, rect.h - 1)] {
            assert_eq!(
                frame.pixel(rect.x + x, rect.y + y),
                view.pixel(x, y),
                "pixel {x},{y} of the pane at {}",
                rect.x
            );
        }
    }
    assert!(has(panes.all()[0], "LEFTPANE") && !has(panes.all()[0], "RIGHTPANE"));
    assert!(has(panes.all()[1], "RIGHTPANE") && !has(panes.all()[1], "LEFTPANE"));
    // The mark is on the focused pane (the new one) only.
    assert_eq!(frame.pixel(right.x + 3, 0), Some(t.prompt));
    assert_ne!(frame.pixel(left.x + 3, 0), Some(t.prompt));
}

#[test]
fn the_focus_moves_between_panes_and_the_mark_follows() {
    let t = theme();
    let mut panes = Panes::new(start("ONE"));
    panes.split(start("TWO"), Split::Down);
    let layout = lay_out(&mut panes);
    assert!(has(panes.focused(), "TWO"), "the new pane has the focus");
    assert!(panes.focus_previous());
    assert!(has(panes.focused(), "ONE"));
    let frame = compose(&panes, &layout);
    assert_eq!(frame.pixel(3, layout.panes[0].y), Some(t.prompt));
    assert_ne!(frame.pixel(3, layout.panes[1].y), Some(t.prompt));
    assert_eq!(layout.panes[1].y, layout.panes[0].h + GAP, "stacked");
}

#[test]
fn closing_a_pane_gives_its_space_back_and_the_last_pane_has_no_divider() {
    let t = theme();
    let mut panes = Panes::new(start("KEPT"));
    panes.split(start("GONE"), Split::Right);
    lay_out(&mut panes);
    let at = panes.focus_index();
    assert!(panes.remove(at).is_some());
    let layout = lay_out(&mut panes);
    assert_eq!(panes.len(), 1);
    assert!(has(panes.focused(), "KEPT"));
    assert_eq!(layout.panes[0].w, WIDTH, "the pane that is left has it all");
    assert_eq!(layout.dividers, vec![]);
    let frame = compose(&panes, &layout);
    assert_eq!(
        frame.pixel(WIDTH / 2, 3),
        panes.focused().view().pixel(WIDTH / 2, 3)
    );
    assert_ne!(
        frame.pixel(3, 0),
        Some(t.prompt),
        "no focus mark on a lone pane"
    );
    assert!(panes.remove(0).is_none(), "the last pane is kept");
}
