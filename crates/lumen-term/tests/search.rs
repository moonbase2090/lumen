//! Find: text found across the output the window keeps, in the scrollback and in
//! rich blocks, with matches drawn and the current one moved by next and
//! previous. A real program on a real PTY, and the real renderer.

#![cfg(unix)]

use lumen_term::design::Palette;
use lumen_term::{
    App, Context, Document, Find, Frame, Key, Layer, Mods, Overlay, Passthrough, Renderer, Scroll,
    Window,
};
use std::time::{Duration, Instant};

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

const COUNT: &str = "i=1; while [ $i -le 300 ]; do echo line$i; i=$((i+1)); done;";

/// A window about 20 rows tall whose program has printed `line1` to `line300`.
fn run(tail: &str) -> App {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &format!("{COUNT} {tail}")],
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

fn type_in(app: &mut App, text: &str) {
    for c in text.chars() {
        app.key(Key::Char(c), Mods::default()).unwrap_or(());
    }
}

fn in_view(app: &App, line: &str) -> bool {
    app.visible_text().iter().any(|l| l == line)
}

/// How many pixels of `colour` the frame has, and the first and last pixel rows
/// they are on.
fn pixels_of(frame: &Frame, colour: [u8; 3]) -> (usize, u32, u32) {
    let (mut n, mut first, mut last) = (0, u32::MAX, 0);
    for y in 0..frame.height {
        for x in 0..frame.width {
            if frame.pixel(x, y) == Some(colour) {
                n += 1;
                first = first.min(y);
                last = last.max(y);
            }
        }
    }
    (n, first, last)
}

#[test]
fn a_hit_a_miss_and_the_match_count() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "line29");
    // line29 and line290 to line299.
    assert_eq!(app.find_count(), 11);
    assert!(app.find_position().is_some());
    // A longer query narrows it: the text is searched as it is typed.
    type_in(&mut app, "9");
    assert_eq!(app.find_count(), 1, "only line299 has 9 after line29");
    app.key(Key::Backspace, Mods::default()).unwrap_or(());
    assert_eq!(app.find_count(), 11);
    type_in(&mut app, "zzz");
    assert_eq!(app.find_count(), 0, "a miss");
    assert_eq!(app.find_position(), None);
    assert_eq!(app.find_current_line(), None);
}

#[test]
fn the_case_of_the_query_does_not_matter() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "LINE300");
    assert_eq!(app.find_count(), 1);
    assert_eq!(app.find_current_line().as_deref(), Some("line300"));
}

#[test]
fn next_and_previous_walk_the_matches_and_wrap() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "line5");
    // line5 and line50 to line59: eleven matches, none below the view.
    assert_eq!(app.find_count(), 11);
    assert_eq!(
        app.find_current_line().as_deref(),
        Some("line59"),
        "with the view at the bottom the match nearest above it is chosen"
    );
    // Next wraps to the first, which is in the scrollback.
    assert!(app.find_next());
    assert_eq!(app.find_current_line().as_deref(), Some("line5"));
    assert_eq!(app.find_position(), Some(0));
    assert!(in_view(&app, "line5"), "the view went to the match");
    assert!(!app.following());
    assert!(app.find_next());
    assert_eq!(app.find_current_line().as_deref(), Some("line50"));
    // Previous goes back, and from the first it wraps to the last.
    assert!(app.find_previous());
    assert_eq!(app.find_current_line().as_deref(), Some("line5"));
    assert!(app.find_previous());
    assert_eq!(app.find_current_line().as_deref(), Some("line59"));
    assert_eq!(app.find_position(), Some(10));
    assert!(in_view(&app, "line59"));
}

#[test]
fn enter_and_shift_enter_are_next_and_previous() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "line5");
    app.key(Key::Enter, Mods::default()).unwrap_or(());
    assert_eq!(app.find_current_line().as_deref(), Some("line5"));
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    app.key(Key::Enter, shift).unwrap_or(());
    assert_eq!(app.find_current_line().as_deref(), Some("line59"));
}

#[test]
fn the_current_match_is_drawn_apart_from_the_others() {
    let mut app = run("sleep 30");
    let p = Palette::light();
    let none = app.view();
    assert_eq!(pixels_of(&none, p.find_current).0, 0);
    assert_eq!(pixels_of(&none, p.find_match).0, 0);

    app.find_open();
    type_in(&mut app, "line29");
    app.find_next();
    let first = app.view();
    let current = pixels_of(&first, p.find_current);
    assert!(current.0 > 0, "the current match is drawn");
    assert!(
        pixels_of(&first, p.find_match).0 > 0,
        "the other matches in view are drawn in the match colour"
    );
    // Next moves the current match to another row.
    app.find_next();
    let second = app.view();
    let moved = pixels_of(&second, p.find_current);
    assert!(moved.0 > 0);
    assert_ne!((current.1, current.2), (moved.1, moved.2), "it moved");
    // Closing the bar takes every highlight away.
    app.key(Key::Escape, Mods::default()).unwrap_or(());
    assert!(!app.finding());
    let closed = app.view();
    assert_eq!(pixels_of(&closed, p.find_current).0, 0);
    assert_eq!(pixels_of(&closed, p.find_match).0, 0);
}

#[test]
fn typing_goes_to_the_bar_and_not_to_the_program() {
    let mut app = run("stty -icanon -echo min 1; cat -v");
    app.find_open();
    type_in(&mut app, "qzx");
    assert_eq!(app.find_query(), "qzx");
    app.key(Key::Escape, Mods::default()).unwrap_or(());
    // Now it is the program's.
    type_in(&mut app, "W");
    assert!(
        pump_until(&mut app, |a| a.visible_text().iter().any(|l| l == "W")),
        "after the bar closes a key reaches the program"
    );
    assert!(
        !app.visible_text().iter().any(|l| l.contains("qzx")),
        "what was typed in the bar never reached it"
    );
}

#[test]
fn new_output_updates_the_matches_and_does_not_move_the_view() {
    let mut app = run("sleep 1; echo line5 again; sleep 30");
    app.find_open();
    type_in(&mut app, "line5");
    let before = app.find_count();
    assert_eq!(before, 11);
    app.find_next();
    let held = app.visible_text();
    assert!(!app.following());
    assert!(pump_until(&mut app, |a| a.find_count() == before + 1));
    assert_eq!(
        app.visible_text(),
        held,
        "the view stayed where the user was"
    );
}

// ---- rich blocks ----

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
fn text_in_rich_blocks_is_found_and_drawn() {
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
    let rows = r.search_rows(&doc, columns);
    assert!(
        rows.iter().any(|x| x.text == "paragraph number 7"),
        "the lines of a rich block are rows"
    );

    let mut find = Find::new();
    find.push("NUMBER 4");
    find.refresh(&rows, 0);
    // number 4 and number 40 to number 49.
    assert_eq!(find.hits().len(), 11);
    assert_eq!(find.current().map(|h| h.row), Some(0));
    let mut find = Find::new();
    find.push("NUMBER");
    find.refresh(&rows, 0);
    assert_eq!(find.hits().len(), 60, "once in each paragraph");
    assert_eq!(find.current_index(), Some(0));

    let total = r.content_height(&doc, columns);
    let (_, ch) = r.cell();
    let area = (WIN.editor_row - 1) * ch;
    let hit = *find.current().unwrap_or_else(|| unreachable!("a match"));
    let scroll = Scroll::to(total, area, hit.top.saturating_sub(2 * ch));
    let overlay = find.overlay();
    let with = r.render_window_scrolled(&doc, &WIN, &[], scroll, (None, Some(&overlay)));
    let without = r.render_window_scrolled(&doc, &WIN, &[], scroll, (None, None));
    let p = Palette::light();
    // The theme the renderer draws with here is the evidence one, whose find
    // colours are the light palette's.
    assert_eq!(r.theme().find_current, p.find_current);
    assert!(pixels_of(&with, p.find_current).0 > 0, "the current match");
    assert!(pixels_of(&with, p.find_match).0 > 0, "the other matches");
    assert_eq!(pixels_of(&without, p.find_current).0, 0);

    // The bar is drawn in the top row of the content, and says how many.
    assert_eq!(overlay.status, "1 of 60");
    let empty = Overlay::default();
    let bar_only = r.render_window_scrolled(&doc, &WIN, &[], scroll, (None, Some(&empty)));
    assert_ne!(
        bar_only, without,
        "an open bar is drawn even before a query"
    );
}

#[test]
fn next_and_previous_after_closing_search_for_the_closed_query() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "line5");
    assert_eq!(app.find_count(), 11);
    app.key(Key::Escape, Mods::default()).unwrap_or(());
    assert!(!app.finding());
    // Find Next with the bar closed opens it on the same query and moves on to a match.
    assert!(app.find_next());
    assert!(app.finding());
    assert_eq!(app.find_query(), "line5");
    assert_eq!(app.find_count(), 11);
    let line = app.find_current_line().unwrap_or_default();
    assert!(line.starts_with("line5"), "on a match, not nothing: {line}");
    assert!(in_view(&app, &line));
    // And Find Previous after another close does the same, the other way.
    app.find_close();
    assert!(app.find_previous());
    assert_eq!(app.find_query(), "line5");
    assert_eq!(app.find_count(), 11);
    assert!(app.find_position().is_some());
}

#[test]
fn reopening_the_bar_shows_the_last_query_with_its_matches() {
    let mut app = run("sleep 30");
    app.find_open();
    type_in(&mut app, "line7");
    app.find_close();
    app.find_open();
    assert_eq!(app.find_query(), "line7");
    assert_eq!(app.find_count(), 11);
    // A bar closed with nothing typed reopens empty.
    let mut empty = run("sleep 30");
    empty.find_open();
    empty.find_close();
    empty.find_open();
    assert_eq!(empty.find_query(), "");
    assert_eq!(empty.find_count(), 0);
}
