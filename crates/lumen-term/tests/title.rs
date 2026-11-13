//! The window title: what a real program sets with OSC 0 and OSC 2 reaches the
//! window chrome with the terminal's name, and an empty or reset title falls
//! back to the name alone. A real program runs behind a real PTY.

#![cfg(unix)]

use lumen_term::APP_NAME;
use lumen_term::title::MAX_TITLE_CHARS;
use lumen_term::{App, Block, Passthrough};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (640, 444);

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

/// Pump until the window title has been `expected.len()` times, in order, or
/// time runs out. Returns every title the chrome was given.
fn titles(app: &mut App, want: usize) -> Vec<String> {
    let mut seen = Vec::new();
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && seen.len() < want {
        app.pump();
        if let Some(t) = app.take_window_title() {
            seen.push(t);
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    seen
}

fn page_text(app: &App) -> String {
    let mut all = String::new();
    for block in app.session().document().blocks() {
        if let Block::Grid(g) = block {
            all.push_str(&g.text_rows().join("\n"));
        }
    }
    all
}

#[test]
fn osc_2_title_reaches_the_chrome_with_the_name() {
    let mut app = start("printf '\\033]2;build: 3 of 9\\007'; sleep 30");
    let got = titles(&mut app, 1);
    assert_eq!(got, vec![format!("build: 3 of 9 - {APP_NAME}")]);
}

#[test]
fn osc_0_title_with_the_string_terminator() {
    let mut app = start("printf '\\033]0;editor\\033\\\\'; sleep 30");
    assert_eq!(titles(&mut app, 1), vec![format!("editor - {APP_NAME}")]);
}

#[test]
fn a_later_title_replaces_an_earlier_one() {
    let mut app =
        start("printf '\\033]2;first\\007'; sleep 1; printf '\\033]2;second\\007'; sleep 30");
    assert_eq!(
        titles(&mut app, 2),
        vec![
            format!("first - {APP_NAME}"),
            format!("second - {APP_NAME}")
        ]
    );
}

#[test]
fn an_empty_title_falls_back_to_the_name_alone() {
    let mut app = start("printf '\\033]2;busy\\007'; sleep 1; printf '\\033]2;\\007'; sleep 30");
    assert_eq!(
        titles(&mut app, 2),
        vec![format!("busy - {APP_NAME}"), APP_NAME.to_owned()]
    );
}

#[test]
fn a_blank_title_falls_back_to_the_name_alone() {
    let mut app = start("printf '\\033]0;   \\007'; sleep 30");
    assert_eq!(titles(&mut app, 1), vec![APP_NAME.to_owned()]);
}

#[test]
fn a_title_with_control_characters_is_ignored() {
    // The second title is refused whole, so the chrome keeps the first.
    let mut app = start(
        "printf '\\033]2;kept\\007'; sleep 1; printf '\\033]2;bad\\001title\\007'; sleep 1; \
         printf '\\033]2;next\\007'; sleep 30",
    );
    assert_eq!(
        titles(&mut app, 2),
        vec![format!("kept - {APP_NAME}"), format!("next - {APP_NAME}")]
    );
}

#[test]
fn direction_controls_do_not_reach_the_chrome() {
    // U+202E (right-to-left override) would reorder the name that follows.
    let mut app = start("printf '\\033]2;abc\\342\\200\\256def\\007'; sleep 30");
    assert_eq!(titles(&mut app, 1), vec![format!("abcdef - {APP_NAME}")]);
}

#[test]
fn a_long_title_is_cut_and_the_name_stays_last() {
    let mut app = start("printf '\\033]2;'; printf 'x%.0s' $(seq 1 600); printf '\\007'; sleep 30");
    let got = titles(&mut app, 1);
    let Some(title) = got.first() else {
        unreachable!("a title arrived")
    };
    assert!(title.ends_with(&format!(" - {APP_NAME}")));
    assert_eq!(
        title.chars().count(),
        MAX_TITLE_CHARS + format!(" - {APP_NAME}").chars().count()
    );
}

#[test]
fn the_title_is_not_drawn_in_the_page() {
    let mut app = start("printf '\\033]2;secret-title\\007visible'; sleep 30");
    assert_eq!(
        titles(&mut app, 1),
        vec![format!("secret-title - {APP_NAME}")]
    );
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !page_text(&app).contains("visible") {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = page_text(&app);
    assert!(
        text.contains("visible"),
        "the program's text is on the page"
    );
    assert!(
        !text.contains("secret-title"),
        "the title is not on the page"
    );
}

#[test]
fn no_title_means_no_change_to_the_chrome() {
    let mut app = start("printf 'plain output'; sleep 30");
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !page_text(&app).contains("plain output") {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.take_window_title().is_none());
}
