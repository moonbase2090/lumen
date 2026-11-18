//! Scrollback: the output the window keeps and the user scrolls, with a real
//! program on a real PTY. The view follows the newest output until the user
//! scrolls up, then holds its place; typing returns to the bottom; a program
//! that asked for the wheel, or that is on the alternate screen, keeps its
//! input.

#![cfg(unix)]

use lumen_term::mouse::{Held, Wheel};
use lumen_term::{
    App, Context, Document, Frame, Key, Layer, Mods, Passthrough, Renderer, Scroll, Window,
};
use std::time::{Duration, Instant};

const NONE: Held = Held {
    shift: false,
    alt: false,
    ctrl: false,
};

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

/// A window about 20 rows tall running `script`, which has printed
/// `line1` to `line300` when this returns, one per row.
fn run(script: &str) -> App {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        640,
        420,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(
        pump_until(&mut app, |a| a
            .visible_text()
            .iter()
            .any(|l| l == "line300")),
        "the program's output arrives"
    );
    app
}

const COUNT: &str = "i=1; while [ $i -le 300 ]; do echo line$i; i=$((i+1)); done;";

fn number(line: &str) -> Option<u32> {
    line.strip_prefix("line")?.parse().ok()
}

fn lines(app: &App) -> Vec<u32> {
    app.visible_text()
        .iter()
        .filter_map(|l| number(l))
        .collect()
}

fn pointer(app: &App) -> (u32, u32) {
    let cell = app.cell_height();
    (200, cell * 3)
}

#[test]
fn the_newest_output_is_in_view_and_the_oldest_is_kept() {
    let app = run(&format!("{COUNT} sleep 30"));
    let shown = lines(&app);
    assert_eq!(
        shown.last(),
        Some(&300),
        "following: the newest is at the bottom"
    );
    assert!(!shown.contains(&1), "the oldest has scrolled out of view");
    assert!(app.following());
}

#[test]
fn page_up_and_page_down_walk_the_output_without_losing_a_line() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    let mut seen = std::collections::BTreeSet::new();
    seen.extend(lines(&app));
    let mut pages = 0;
    while !app.at_top() {
        app.key(Key::PageUp, Mods::default()).unwrap_or(());
        seen.extend(lines(&app));
        pages += 1;
        assert!(pages < 100, "page up reaches the top");
    }
    assert!(!app.following());
    assert_eq!(lines(&app).first(), Some(&1), "the top is the first line");
    assert_eq!(
        seen.iter().copied().collect::<Vec<_>>(),
        (1..=300).collect::<Vec<_>>(),
        "every line was in view on the way up"
    );
    while !app.following() {
        app.key(Key::PageDown, Mods::default()).unwrap_or(());
        pages -= 1;
        assert!(pages > -100, "page down reaches the bottom");
    }
    assert_eq!(lines(&app).last(), Some(&300));
}

#[test]
fn the_wheel_scrolls_three_rows_a_step_and_a_trackpad_by_its_pixels() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    let (x, y) = pointer(&app);
    let find = |app: &App, n: u32| app.visible_text().iter().position(|l| number(l) == Some(n));
    let before = find(&app, 295).unwrap_or(usize::MAX);
    assert!(app.mouse_wheel(x, y, Wheel::Up, NONE).unwrap_or(false));
    let after = find(&app, 295).unwrap_or(usize::MAX);
    assert_eq!(after, before + 3, "the same line is three rows lower");
    assert!(!app.following());

    let Scroll::Pinned(held) = app.scroll() else {
        unreachable!("scrolled up is pinned");
    };
    assert!(app.wheel_pixels(x, y, 7, NONE).unwrap_or(false));
    assert_eq!(
        app.scroll(),
        Scroll::Pinned(held - 7),
        "seven pixels, exactly"
    );
    assert!(app.wheel_pixels(x, y, -7, NONE).unwrap_or(false));
    assert_eq!(app.scroll(), Scroll::Pinned(held));

    for _ in 0..50 {
        app.mouse_wheel(x, y, Wheel::Down, NONE).unwrap_or(false);
    }
    assert!(
        app.following(),
        "wheeling back down to the bottom follows again"
    );
    assert_eq!(lines(&app).last(), Some(&300));
}

#[test]
fn new_output_does_not_move_a_view_that_was_scrolled_up() {
    let mut app = run(&format!(
        "{COUNT} sleep 3; i=301; while [ $i -le 350 ]; do echo line$i; i=$((i+1)); done; sleep 30"
    ));
    for _ in 0..3 {
        app.key(Key::PageUp, Mods::default()).unwrap_or(());
    }
    let held = app.visible_text();
    let place = app.scroll();
    assert!(!app.following());
    assert!(
        pump_until(&mut app, |a| a.session().document().blocks().iter().any(
            |b| match b {
                lumen_term::Block::Grid(g) => g.text_rows().iter().any(|l| l == "line350"),
                lumen_term::Block::Rich(_) => false,
            }
        )),
        "the program wrote 50 more lines while the view was scrolled up"
    );
    assert_eq!(app.visible_text(), held, "the rows in view did not change");
    assert_eq!(app.scroll(), place);
    assert!(!app.following());
    app.key(
        Key::End,
        Mods {
            shift: true,
            ..Mods::default()
        },
    )
    .unwrap_or(());
    assert!(app.following());
    assert_eq!(
        lines(&app).last(),
        Some(&350),
        "back at the bottom: the new output"
    );
}

#[test]
fn typing_returns_to_the_newest_output() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    app.key(Key::PageUp, Mods::default()).unwrap_or(());
    app.key(Key::PageUp, Mods::default()).unwrap_or(());
    assert!(!app.following());
    app.key(Key::Char('x'), Mods::default()).unwrap_or(());
    assert!(app.following());
    assert_eq!(lines(&app).last(), Some(&300));
}

#[test]
fn a_program_that_asked_for_the_wheel_keeps_it() {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &[
            "-c",
            &format!("{COUNT} printf '\\033[?1000h\\033[?1006h'; stty -icanon -echo min 1; cat -v"),
        ],
        1,
        640,
        420,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |a| a
        .visible_text()
        .iter()
        .any(|l| l == "line300")));
    let _ = pump_until(&mut app, |_| false);
    let (x, y) = pointer(&app);
    // Over the live block the wheel is the program's.
    let ch = app.cell_height();
    let rows = u32::try_from(app.visible_text().len()).unwrap_or(1);
    let over_live = (x, ch + (rows - 2) * ch);
    assert!(
        app.mouse_wheel(over_live.0, over_live.1, Wheel::Up, NONE)
            .unwrap_or(false)
    );
    assert!(
        app.following(),
        "the program took the wheel, the view did not move"
    );
    let _ = y;
    assert!(
        pump_until(&mut app, |a| a
            .visible_text()
            .iter()
            .any(|l| l.contains("[<64;"))),
        "the program received the wheel report"
    );
}

#[test]
fn a_program_on_the_alternate_screen_keeps_page_up() {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &[
            "-c",
            &format!("{COUNT} printf '\\033[?1049h'; stty -icanon -echo min 1; cat -v"),
        ],
        1,
        640,
        420,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let _ = pump_until(&mut app, |a| {
        a.session()
            .document()
            .blocks()
            .iter()
            .any(|b| matches!(b, lumen_term::Block::Grid(g) if g.alt_active()))
    });
    app.key(Key::PageUp, Mods::default()).unwrap_or(());
    assert!(app.following(), "Page Up did not scroll the output");
    assert!(
        pump_until(&mut app, |a| a
            .visible_text()
            .iter()
            .any(|l| l.contains("^[[5~"))),
        "the program received Page Up"
    );
}

const WIN: Window = Window {
    cols: 80,
    rows: 24,
    editor_row: 19,
    editor_rows: 2,
};

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame_paragraph(text: &str) -> Vec<u8> {
    let body = string(text);
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&lumen_detector::TAG);
    v.push(0x80);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(&body);
    v
}

#[test]
fn rich_blocks_stay_whole_when_scrolled_to() {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the face loads: {e}"));
    let mut doc = Document::new(76, 18);
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut layer);
    let mut bytes = Vec::new();
    for i in 0..60 {
        bytes.extend(frame_paragraph(&format!("paragraph number {i}")));
    }
    doc.receive(&mut layer, &bytes, Context::default());
    let columns = usize::try_from(WIN.cols - 4).unwrap_or(1);
    let total = r.content_height(&doc, columns);
    let (_, ch) = r.cell();
    let area = (WIN.editor_row - 1) * ch;
    assert!(total > area * 3, "the flow is taller than the view");

    let bottom = r.render_window_scrolled(&doc, &WIN, &[], Scroll::Follow, (None, None));
    let top = r.render_window_scrolled(&doc, &WIN, &[], Scroll::to_top(total, area), (None, None));
    assert_ne!(bottom, top, "scrolling changes what is drawn");

    // The first block at the top of the flow is drawn exactly as it is on its own.
    let first = doc.blocks().iter().find_map(|b| match b {
        lumen_term::Block::Rich(x) if !x.background => Some(x),
        _ => None,
    });
    let first = first.unwrap_or_else(|| unreachable!("a rich block"));
    let mut alone = Frame::new(
        u32::try_from(columns).unwrap_or(1) * r.cell().0,
        r.rich_height(first, columns),
        r.theme().background,
    );
    r.paint_rich(&mut alone, 0, 0, columns, first);
    let (x0, y0) = (4 * r.cell().0 + r.gutter_pad(), ch);
    // The scroll bar is the terminal's, drawn over the right edge of the area.
    let bar = 2 * r.scale();
    for y in 0..alone.height {
        for x in 0..alone.width - bar {
            assert_eq!(
                top.pixel(x0 + x, y0 + y),
                alone.pixel(x, y),
                "pixel {x},{y} of the first block"
            );
        }
    }
}

/// Take frames until the glide has given everything it holds.
fn settle(app: &mut App) {
    for _ in 0..2000 {
        if !app.is_gliding() {
            return;
        }
        app.tick();
    }
    unreachable!("the glide never finished");
}

#[test]
fn a_wheel_step_eases_the_view_three_rows_up_over_several_frames() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    let (x, y) = pointer(&app);
    let row = i64::from(app.cell_height());
    assert!(app.following());
    assert!(
        app.mouse_wheel_glide(x, y, Wheel::Up, NONE)
            .unwrap_or(false),
        "the step asked for a move"
    );
    assert!(app.following(), "nothing has moved before the first frame");
    assert!(app.is_gliding());
    // Frame by frame the view never moves past where three rows would take it.
    let mut frames = 0;
    let mut offsets = Vec::new();
    while app.is_gliding() && frames < 2000 {
        app.tick();
        frames += 1;
        offsets.push(match app.scroll() {
            Scroll::Pinned(top) => top,
            Scroll::Follow => 0,
        });
    }
    assert!(!app.is_gliding());
    assert!(frames > 3, "it took more than one frame: {frames}");
    let Scroll::Pinned(end) = app.scroll() else {
        unreachable!("scrolled up is pinned")
    };
    let first = *offsets.first().unwrap_or(&0);
    let moved = i64::from(first) - i64::from(end);
    assert!(
        moved < 3 * row,
        "the first frame moved only part of it: {moved}"
    );
    // The same three rows a jump makes.
    let mut jump = run(&format!("{COUNT} sleep 30"));
    assert!(jump.mouse_wheel(x, y, Wheel::Up, NONE).unwrap_or(false));
    assert_eq!(jump.scroll(), app.scroll(), "it ends where a jump ends");
}

#[test]
fn easing_ends_at_the_top_and_does_not_hold_a_move_it_cannot_make() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    let (x, y) = pointer(&app);
    assert!(app.scroll_end(true));
    assert!(app.at_top());
    assert!(
        !app.mouse_wheel_glide(x, y, Wheel::Up, NONE).unwrap_or(true),
        "nothing to ease at the top"
    );
    assert!(!app.is_gliding());
    // One step down from the top eases and arrives.
    assert!(
        app.mouse_wheel_glide(x, y, Wheel::Down, NONE)
            .unwrap_or(false)
    );
    settle(&mut app);
    assert!(!app.at_top());
}

#[test]
fn typing_and_other_scrolling_stop_an_easing_view() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    let (x, y) = pointer(&app);
    assert!(
        app.mouse_wheel_glide(x, y, Wheel::Up, NONE)
            .unwrap_or(false)
    );
    app.tick();
    assert!(app.is_gliding());
    assert!(app.scroll_page(true));
    assert!(!app.is_gliding(), "a page move ends the easing");
    let held = app.scroll();
    app.tick();
    assert_eq!(app.scroll(), held, "and nothing moves after it");

    assert!(
        app.mouse_wheel_glide(x, y, Wheel::Up, NONE)
            .unwrap_or(false)
    );
    app.key(Key::Char('x'), Mods::default()).unwrap_or(());
    assert!(!app.is_gliding(), "typing ends the easing");
    assert!(app.following());
    app.tick();
    assert!(app.following(), "and the view stays at the bottom");
}

#[test]
fn with_smooth_scrolling_off_a_wheel_step_jumps() {
    let mut app = run(&format!("{COUNT} sleep 30"));
    app.set_smooth_scroll(false);
    let (x, y) = pointer(&app);
    assert!(
        app.mouse_wheel_glide(x, y, Wheel::Up, NONE)
            .unwrap_or(false)
    );
    assert!(!app.is_gliding());
    assert!(!app.following(), "the view moved at once");
}
