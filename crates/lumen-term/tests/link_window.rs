//! Links: a web address written in a program's output can be activated, the
//! terminal asks first and shows the whole target, and nothing opens until the
//! user says yes. A link a program sets with OSC 8 is one too, and the
//! question shows the real target beside the text it covers. A real program runs behind a real PTY; the
//! system opener is replaced by one that records what it was given.

#![cfg(all(unix, feature = "links-clipboard"))]

use lumen_term::linkprompt::LinkOpener;
use lumen_term::{App, Key, Passthrough};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (800, 480);

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
    fn count(&self) -> usize {
        self.all().len()
    }

    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn start(script: &str) -> (App, Opened) {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let opened = Opened::default();
    app.use_link_opener(Box::new(opened.clone()));
    (app, opened)
}

/// Take the program's output as it arrives until `done`. Each wait ends when
/// the program writes; the limit only ends a test that never gets its signal.
fn pump_until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
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

fn shows(app: &App, text: &str) -> bool {
    app.visible_text().iter().any(|row| row.contains(text))
}

/// A pixel over the first cell of the row of the live grid that shows `text`,
/// `offset` characters in.
fn pixel_of(app: &App, text: &str, offset: usize) -> (u32, u32) {
    let rows = app.visible_text();
    let row = rows
        .iter()
        .position(|r| r.contains(text))
        .unwrap_or_else(|| unreachable!("{text} is on screen: {rows:?}"));
    let col = rows[row].find(text).unwrap_or(0) + offset;
    let step = (app.cell_height() / 2).max(1);
    for y in (0..SIZE.1).step_by(step as usize) {
        for x in (0..SIZE.0).step_by(2) {
            if let Some((c, r)) = app.live_cell(x, y)
                && c as usize == col
                && r as usize == row - first_grid_row(&rows, &app.visible_text())
            {
                return (x, y);
            }
        }
    }
    unreachable!("no pixel for {text}");
}

/// The visible rows start at the grid's first row; the grid is the whole page
/// here, so its first row is the first visible row.
fn first_grid_row(_: &[String], _: &[String]) -> usize {
    0
}

fn lines(app: &App) -> String {
    app.link_prompt()
        .map(|p| p.lines().join("\n"))
        .unwrap_or_default()
}

const SAY: &str = "echo 'docs at https://example.com/guide?a=1 and more'; sleep 30";
const URL: &str = "https://example.com/guide?a=1";

fn enter(app: &mut App) {
    app.key(Key::Enter, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_address_in_the_output_asks_with_the_whole_target_and_opens_only_on_yes() {
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    let (x, y) = pixel_of(&app, URL, 10);

    assert!(app.link_press(x, y), "the press is the terminal's");
    assert!(lines(&app).contains(URL), "{}", lines(&app));
    assert!(lines(&app).contains("Found in the program's output"));
    assert!(opened.all().is_empty(), "nothing opens before the answer");

    enter(&mut app);
    assert_eq!(opened.all(), vec![URL]);
    assert!(app.link_prompt().is_none());
}

#[test]
fn cancelling_opens_nothing_and_the_program_sees_none_of_it() {
    let (mut app, opened) = start(
        "echo 'go https://example.com/x now'; read line; printf 'GOT[%s]\\n' \"$line\"; sleep 30",
    );
    assert!(pump_until(&mut app, |a| shows(a, "now")));
    let (x, y) = pixel_of(&app, "https://example.com/x", 2);
    assert!(app.link_press(x, y));

    // Keys while the prompt is up belong to the prompt.
    for c in ['x', 'y'] {
        app.key(Key::Char(c), lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    assert!(app.link_prompt().is_some(), "other keys do not answer it");
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.count(), 0);

    // What the user types after it reaches the program, and only that.
    for c in ['o', 'k'] {
        app.key(Key::Char(c), lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    enter(&mut app);
    assert!(pump_until(&mut app, |a| shows(a, "GOT[ok]")));
}

#[test]
fn the_buttons_answer_by_click_and_a_click_away_cancels() {
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    let (x, y) = pixel_of(&app, URL, 0);

    assert!(app.link_press(x, y));
    let b = app
        .link_prompt_box()
        .unwrap_or_else(|| unreachable!("a prompt"));
    let open = b
        .buttons
        .iter()
        .find(|(l, _)| l.contains("Open"))
        .map(|(_, r)| *r);
    let (bx, by, _, _) = open.unwrap_or_else(|| unreachable!("an open button"));
    assert!(app.prompt_click(bx + 2, by + 2));
    assert_eq!(opened.all().len(), 1);

    assert!(app.link_press(x, y));
    let b = app
        .link_prompt_box()
        .unwrap_or_else(|| unreachable!("a prompt"));
    let cancel = b
        .buttons
        .iter()
        .find(|(l, _)| l.contains("Cancel"))
        .map(|(_, r)| *r);
    let (cx, cy, _, _) = cancel.unwrap_or_else(|| unreachable!("a cancel button"));
    assert!(app.prompt_click(cx + 2, cy + 2));
    assert_eq!(opened.all().len(), 1, "cancel opens nothing");

    assert!(app.link_press(x, y));
    assert!(app.prompt_click(0, 0), "a click away closes it");
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.all().len(), 1);
}

#[test]
fn a_press_off_any_link_is_left_for_the_program() {
    let (mut app, _) = start("echo 'no link on this row'; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "no link")));
    let (x, y) = pixel_of(&app, "no link", 0);
    assert!(!app.link_press(x, y));
    assert!(app.link_prompt().is_none());
}

/// A shell command that prints `text` as an OSC 8 link to `target`, closed,
/// then `AFTER`, and waits.
fn osc8(target: &str, text: &str) -> String {
    format!("printf '\\033]8;;{target}\\033\\\\{text}\\033]8;;\\033\\\\ AFTER\\n'; sleep 30")
}

#[test]
fn a_link_a_program_sets_shows_its_real_target_and_the_text_it_covers_and_opens_only_on_yes() {
    let (mut app, opened) = start(&osc8("https://real.example/docs?a=1", "read the docs"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "read the docs", 5);
    assert!(app.link_press(x, y), "the press is the terminal's");
    let prompt = lines(&app);
    assert!(prompt.contains("https://real.example/docs?a=1"), "{prompt}");
    assert!(prompt.contains("Set by the program"), "{prompt}");
    assert!(prompt.contains("Text shown: read the docs"), "{prompt}");
    assert!(prompt.contains("Host: real.example"), "{prompt}");
    assert!(opened.all().is_empty(), "nothing opens before the answer");
    enter(&mut app);
    assert_eq!(opened.all(), vec!["https://real.example/docs?a=1"]);
}

#[test]
fn a_program_link_whose_text_is_another_address_warns_and_opens_what_it_shows() {
    let (mut app, opened) = start(&osc8(
        "https://evil.example/login",
        "https://bank.example/login",
    ));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "https://bank.example/login", 3);
    assert!(app.link_press(x, y));
    let prompt = lines(&app);
    assert!(prompt.contains("https://evil.example/login"), "{prompt}");
    assert!(
        prompt.contains("Warning: the text shown looks like"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Text shown: https://bank.example/login"),
        "{prompt}"
    );
    enter(&mut app);
    assert_eq!(opened.all(), vec!["https://evil.example/login"]);
}

#[test]
fn cancelling_a_program_link_opens_nothing() {
    let (mut app, opened) = start(&osc8("https://real.example/", "go"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "go", 0);
    assert!(app.link_press(x, y));
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.count(), 0);
}

#[test]
fn a_program_link_to_a_scheme_that_is_not_opened_says_so_and_opens_nothing() {
    for target in [
        "javascript:alert(1)",
        "ftp://real.example/",
        "file:///etc/hosts",
    ] {
        let (mut app, opened) = start(&osc8(target, "click"));
        assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
        let (x, y) = pixel_of(&app, "click", 1);
        assert!(app.link_press(x, y), "{target} still asks, to explain");
        enter(&mut app);
        assert!(opened.all().is_empty(), "{target} opened");
    }
}

#[test]
fn text_after_a_program_link_ends_is_not_part_of_it() {
    let (mut app, _) = start(&osc8("https://real.example/", "inside"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "AFTER", 1);
    assert!(
        !app.link_press(x, y),
        "the text after the link is not a link"
    );
    assert!(app.link_prompt().is_none());
}

#[test]
fn a_program_link_over_the_limits_is_not_a_link_and_does_not_keep_an_earlier_one() {
    // The first link is closed by the over-long one, which is not passed on.
    let long = "x".repeat(2100);
    let (mut app, _) = start(&format!(
        "printf '\\033]8;;https://first.example/\\033\\\\one \\033]8;;https://real.example/{long}\\033\\\\two\\033]8;;\\033\\\\ AFTER\\n'; sleep 30"
    ));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "one", 1);
    assert!(app.link_press(x, y), "the first link is a link");
    assert!(lines(&app).contains("https://first.example/"));
    assert!(app.answer_link(false).is_ok());
    let (x, y) = pixel_of(&app, "two", 1);
    assert!(
        !app.link_press(x, y),
        "the over-long link is not one, and the first has ended"
    );
}

#[test]
fn a_program_link_is_underlined_in_the_accent_colour() {
    let (mut app, _) = start(&osc8("https://real.example/", "underlined text"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of(&app, "underlined text", 3);
    let ch = app.cell_height();
    let cell_bottom = y - y % ch + ch;
    let accent = lumen_term::design::theme().prompt;
    let frame = app.view();
    let found =
        (cell_bottom.saturating_sub(4)..cell_bottom).any(|row| frame.pixel(x, row) == Some(accent));
    assert!(found, "an accent underline under the program's link");
}

#[test]
fn only_web_addresses_in_text_are_links() {
    let (mut app, _) = start(
        "echo 'mail:a@lumenprotocol.org file:///etc/hosts ftp://example.com javascript:alert(1)'; sleep 30",
    );
    assert!(pump_until(&mut app, |a| shows(a, "javascript")));
    for word in [
        "a@lumenprotocol.org",
        "file:///etc/hosts",
        "ftp://example.com",
        "alert",
    ] {
        let (x, y) = pixel_of(&app, word, 2);
        assert!(!app.link_press(x, y), "{word} is not a link");
    }
}

#[test]
fn an_address_with_a_user_name_in_it_is_shown_and_refused() {
    let (mut app, opened) = start("echo 'see https://user:pw@lumenprotocol.org/ here'; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "here")));
    let (x, y) = pixel_of(&app, "https://user:pw@lumenprotocol.org/", 8);
    assert!(app.link_press(x, y));
    assert!(lines(&app).contains("This link is not opened."));
    enter(&mut app);
    assert_eq!(opened.count(), 0);
}

#[test]
fn the_prompt_is_drawn_over_the_page_and_changes_only_its_box() {
    let (mut app, _) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    let before = app.view();
    let (x, y) = pixel_of(&app, URL, 0);
    assert!(app.link_press(x, y));
    let after = app.view();
    // With a directory named, the two frames are kept as PNG for a proof.
    if let Some(dir) = std::env::var_os("LUMEN_PROOF_DIR") {
        for (name, frame) in [("link-before.png", &before), ("link-prompt.png", &after)] {
            let png = frame.to_png().unwrap_or_else(|e| unreachable!("png: {e}"));
            std::fs::write(std::path::Path::new(&dir).join(name), png)
                .unwrap_or_else(|e| unreachable!("write: {e}"));
        }
    }
    let b = app
        .link_prompt_box()
        .unwrap_or_else(|| unreachable!("a prompt"));
    let (bx, by, bw, bh) = b.rect;
    assert!(
        bx + bw <= SIZE.0 && by + bh <= SIZE.1,
        "the box is in the window"
    );
    assert!(
        (by..by + bh)
            .any(|row| (bx..bx + bw).any(|col| before.pixel(col, row) != after.pixel(col, row))),
        "the prompt changes the pixels in its box"
    );
    for (px, py) in [(2, SIZE.1 - 2), (SIZE.0 - 2, 2)] {
        assert_eq!(
            before.pixel(px, py),
            after.pixel(px, py),
            "away from the box"
        );
    }
}

#[test]
fn a_written_address_is_underlined_in_the_accent_colour() {
    let (mut app, _) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    let (x, y) = pixel_of(&app, URL, 3);
    // The cell's bottom pixel rows: one has the underline.
    let ch = app.cell_height();
    let cell_bottom = y - y % ch + ch;
    let accent = lumen_term::design::theme().prompt;
    let frame = app.view();
    let found =
        (cell_bottom.saturating_sub(4)..cell_bottom).any(|row| frame.pixel(x, row) == Some(accent));
    assert!(found, "an accent underline under the address");
}

#[test]
fn a_soft_wrapped_address_is_underlined_and_asks_with_the_whole_target_on_each_row() {
    const ADDRESS: &str = "https://example.org/a/long/path/that/goes/past/the/edge";
    const WIDTH: u32 = 420;
    const HEIGHT: u32 = 300;
    let script = format!("printf '%25s' ''; printf '%s\\r\\n' '{ADDRESS} END'; read hold");
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        WIDTH,
        HEIGHT,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let opened = Opened::default();
    app.use_link_opener(Box::new(opened.clone()));
    assert!(pump_until(&mut app, |a| shows(a, " END")));
    let columns = app
        .session()
        .document()
        .blocks()
        .iter()
        .find_map(|block| match block {
            lumen_term::Block::Grid(grid) => Some(grid.size().0),
            lumen_term::Block::Rich(_) => None,
        })
        .unwrap_or_else(|| unreachable!("a live grid"));
    let mut positions = std::collections::BTreeMap::new();
    for y in (0..HEIGHT).step_by(2) {
        for x in (0..WIDTH).step_by(2) {
            if let Some((col, row)) = app.live_cell(x, y) {
                positions.entry((row, col)).or_insert((x, y));
            }
        }
    }
    let frame = app.view();
    let accent = lumen_term::design::theme().prompt;
    let mut clickable_rows = std::collections::BTreeMap::new();
    for ((row, col), (x, y)) in positions {
        let address_cell =
            (25..25 + ADDRESS.len()).contains(&(row as usize * columns + col as usize));
        let hit = app.link_at(x, y);
        assert_eq!(
            hit.as_ref().map(|h| h.target.as_str()),
            address_cell.then_some(ADDRESS),
            "cell {row}:{col}"
        );
        if address_cell {
            let bottom = y - y % app.cell_height() + app.cell_height();
            assert!(
                (bottom.saturating_sub(4)..bottom).any(|line| frame.pixel(x, line) == Some(accent)),
                "underline at {row}:{col}"
            );
            clickable_rows.entry(row).or_insert((x, y));
        }
    }
    assert!(
        clickable_rows.len() >= 2,
        "the address must cross a real row edge"
    );
    for &(x, y) in clickable_rows.values() {
        assert!(app.link_press(x, y));
        assert!(lines(&app).contains(ADDRESS), "{}", lines(&app));
        assert_eq!(opened.count(), 0);
        app.key(Key::Escape, lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let &(x, y) = clickable_rows
        .values()
        .last()
        .unwrap_or_else(|| unreachable!("wrapped link"));
    assert!(app.link_press(x, y));
    if let Some(dir) = std::env::var_os("LUMEN_PROOF_DIR") {
        for (name, image) in [
            ("wrapped-link.png", frame),
            ("wrapped-link-prompt.png", app.view()),
        ] {
            std::fs::write(
                std::path::Path::new(&dir).join(name),
                image.to_png().unwrap_or_else(|e| unreachable!("png: {e}")),
            )
            .unwrap_or_else(|e| unreachable!("write: {e}"));
        }
    }
    enter(&mut app);
    assert_eq!(opened.all(), vec![ADDRESS]);
    eprintln!(
        "wrapped link: every target cell is clickable and underlined; {} rows ask with the full target; opening uses the full target",
        clickable_rows.len()
    );
}

#[test]
fn the_pixel_wheel_is_the_prompts_too_while_it_is_up() {
    // The program asks for mouse reports and shows what it is sent, so a
    // wheel report that reached it would be on screen.
    let (mut app, _) = start(
        "printf '\\033[?1000h\\033[?1006h\\033[?1h'; echo 'at https://example.com/w here'; stty raw -echo; printf 'READY\\r\\n'; cat -v",
    );
    // READY is written once the program has the terminal raw: input before that
    // would be echoed twice by the line discipline.
    assert!(pump_until(&mut app, |a| shows(a, "READY")));
    let (x, y) = pixel_of(&app, "https://example.com/w", 2);

    // Before the prompt, a trackpad wheel reaches the program.
    assert!(
        app.wheel_pixels(x, y, 40, lumen_term::mouse::Held::default())
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    // The program's output keeps the order it was sent in, so a marker typed
    // after the wheel is on screen only once the wheel's report is.
    app.text("Q").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(pump_until(&mut app, |a| shows(a, "^[[<64;") && shows(a, "Q")));

    assert!(app.link_press(x, y));
    let before = app.visible_text();
    for pixels in [40, -40, 400] {
        assert!(
            !app.wheel_pixels(x, y, pixels, lumen_term::mouse::Held::default())
                .unwrap_or_else(|e| unreachable!("{e}")),
            "the pixel wheel is the prompt's while it is up"
        );
    }
    assert!(app.link_prompt().is_some());

    // Answer the question, then send a second marker: when it is on screen,
    // anything the wheel had sent the program would be too, before it.
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    app.text("Z").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(pump_until(&mut app, |a| shows(a, "Z")));
    let after: Vec<String> = app
        .visible_text()
        .iter()
        .map(|line| line.replace('Z', ""))
        .collect();
    assert_eq!(after, before, "the program saw nothing");
}

#[test]
fn an_address_in_the_scrollback_is_underlined_and_activatable() {
    let (mut app, opened) =
        start("echo 'old https://example.com/old end'; seq 1 200; echo tail; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "tail")));
    assert!(
        !shows(&app, "https://example.com/old"),
        "it has scrolled off"
    );
    assert!(app.scroll_end(true), "scrolled to the top");

    let found = (0..SIZE.1)
        .step_by(2)
        .flat_map(|y| (0..SIZE.0).step_by(2).map(move |x| (x, y)))
        .find(|&(x, y)| app.link_at(x, y).is_some())
        .unwrap_or_else(|| unreachable!("no link in the scrollback"));
    let hit = app.link_at(found.0, found.1);
    assert_eq!(
        hit.as_ref().map(|h| h.target.as_str()),
        Some("https://example.com/old")
    );

    // It is drawn underlined.
    let accent = lumen_term::design::theme().prompt;
    let frame = app.view();
    let ch = app.cell_height();
    let bottom = found.1 - found.1 % ch + ch;
    assert!(
        (bottom.saturating_sub(4)..bottom).any(|row| frame.pixel(found.0, row) == Some(accent)),
        "an accent underline under the address in the history"
    );

    // And it asks, and opens only on yes.
    assert!(app.link_press(found.0, found.1));
    assert!(lines(&app).contains("https://example.com/old"));
    assert_eq!(opened.count(), 0);
    enter(&mut app);
    assert_eq!(opened.all(), vec!["https://example.com/old"]);
}

/// A pixel over the link whose target is `target`, wherever the view shows it.
fn pixel_of_link(app: &App, target: &str) -> (u32, u32) {
    (0..SIZE.1)
        .step_by(2)
        .flat_map(|y| (0..SIZE.0).step_by(2).map(move |x| (x, y)))
        .find(|&(x, y)| app.link_at(x, y).is_some_and(|h| h.target == target))
        .unwrap_or_else(|| unreachable!("no link to {target} in view"))
}

#[test]
fn a_program_link_in_the_scrollback_is_underlined_and_activatable() {
    let (mut app, opened) = start(
        "printf '\\033]8;;https://old.example/\\033\\\\first\\033]8;;\\033\\\\ and \\033]8;;https://second.example/\\033\\\\second\\033]8;;\\033\\\\\\n'; seq 1 200; echo tail; sleep 30",
    );
    assert!(pump_until(&mut app, |a| shows(a, "tail")));
    assert!(!shows(&app, "first"), "it has scrolled off");
    assert!(app.scroll_end(true), "scrolled to the top");
    let (x, y) = pixel_of_link(&app, "https://old.example/");
    let accent = lumen_term::design::theme().prompt;
    let frame = app.view();
    let ch = app.cell_height();
    let bottom = y - y % ch + ch;
    assert!(
        (bottom.saturating_sub(4)..bottom).any(|row| frame.pixel(x, row) == Some(accent)),
        "an accent underline under the program's link in the history"
    );
    assert!(app.link_press(x, y));
    assert!(lines(&app).contains("https://old.example/"));
    assert!(lines(&app).contains("Set by the program"));
    enter(&mut app);
    // The second link on the same line is its own target.
    let (x, y) = pixel_of_link(&app, "https://second.example/");
    assert!(app.link_press(x, y));
    enter(&mut app);
    assert_eq!(
        opened.all(),
        vec!["https://old.example/", "https://second.example/"]
    );
}

struct Clip(&'static str);

impl lumen_term::clipboard::Clipboard for Clip {
    fn text(&self) -> Option<String> {
        Some(self.0.to_owned())
    }
}

#[test]
fn a_menu_or_shortcut_action_does_nothing_while_the_prompt_is_up() {
    use lumen_term::actions::Action;
    use lumen_term::app::Outcome;
    let (mut app, _) = start(
        "echo 'at https://example.com/p here'; printf '\\033[?1h'; stty raw -echo; printf 'READY\\r\\n'; cat -v",
    );
    assert!(pump_until(&mut app, |a| shows(a, "READY")));
    // The keys and paste are the program's once it sets application keys (otherwise the input box takes them); before that they
    // would land in the input box.
    assert!(pump_until(&mut app, |a| a.input_mode()
        == lumen_term::input_line::Mode::Raw));
    let (x, y) = pixel_of(&app, "https://example.com/p", 2);

    // With no prompt, Paste reaches the program.
    assert_eq!(
        app.perform(Action::Paste, &Clip("first-paste")),
        Outcome::Redraw
    );
    assert!(pump_until(&mut app, |a| shows(a, "first-paste")));

    assert!(app.link_press(x, y));
    for action in [
        Action::Paste,
        Action::Find,
        Action::SelectAll,
        Action::ZoomIn,
    ] {
        assert_eq!(
            app.perform(action, &Clip("second-paste")),
            Outcome::Nothing,
            "{action:?} is the prompt's while it is up"
        );
    }
    assert!(app.link_prompt().is_some());

    // Close still works, and after the answer Paste reaches the program again.
    assert_eq!(app.perform(Action::Close, &Clip("")), Outcome::Exit);
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        app.perform(Action::Paste, &Clip("third-paste")),
        Outcome::Redraw
    );
    assert!(pump_until(&mut app, |a| shows(a, "third-paste")));
    // Output keeps its order: the second paste would be on screen by now.
    assert!(!shows(&app, "second-paste"), "the program saw nothing");
}

#[test]
fn the_button_labels_are_readable_in_every_built_in_palette() {
    use lumen_term::design::{Builtin, ThemeChoice, contrast};
    // The text floor each palette keeps, written out here.
    let palettes = [
        (Builtin::Light, 4.5),
        (Builtin::Dark, 4.5),
        (Builtin::HighContrast, 7.0),
    ];
    let (mut app, _) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    let (x, y) = pixel_of(&app, URL, 0);
    assert!(app.link_press(x, y));
    for (builtin, floor) in palettes {
        let _ = app.set_theme_choice(ThemeChoice::Fixed(builtin.palette()));
        let frame = app.view();
        let b = app
            .link_prompt_box()
            .unwrap_or_else(|| unreachable!("a prompt"));
        for (label, (bx, by, bw, bh)) in &b.buttons {
            // Inside the button's one-pixel edge, left of its label.
            let fill = frame.pixel(bx + 2, by + 2).unwrap_or_default();
            let mut ink = 1.0_f64;
            for py in *by..by + bh {
                for px in *bx..bx + bw {
                    if let Some(c) = frame.pixel(px, py) {
                        ink = ink.max(contrast(c, fill));
                    }
                }
            }
            assert!(
                ink >= floor,
                "{builtin:?}: {label:?} is {ink:.2}:1 against its button, under {floor}:1"
            );
        }
    }
}

struct NoClip;

impl lumen_term::clipboard::Clipboard for NoClip {
    fn text(&self) -> Option<String> {
        None
    }
}

fn act(app: &mut App, action: lumen_term::actions::Action) {
    app.perform(action, &NoClip);
}

const TWO: &str = "echo 'one https://example.com/a two https://example.com/b'; echo 'three https://example.com/c'; sleep 30";

#[test]
fn the_keyboard_walks_the_links_in_reading_order_and_wraps() {
    use lumen_term::actions::Action;
    let (mut app, _) = start(TWO);
    assert!(pump_until(&mut app, |a| shows(a, "example.com/c")));
    let mut seen = Vec::new();
    for _ in 0..4 {
        act(&mut app, Action::NextLink);
        seen.push(app.focused_link().map(|h| h.target.clone()));
    }
    let want = |s: &str| Some(format!("https://example.com/{s}"));
    assert_eq!(seen, [want("a"), want("b"), want("c"), want("a")]);
    act(&mut app, Action::PreviousLink);
    assert_eq!(app.focused_link().map(|h| h.target.clone()), want("c"));
}

#[test]
fn the_focused_link_is_shown_selected_and_copies_as_its_text() {
    use lumen_term::actions::Action;
    let (mut app, _) = start(TWO);
    assert!(pump_until(&mut app, |a| shows(a, "example.com/c")));
    act(&mut app, Action::NextLink);
    assert_eq!(
        app.selected_text().as_deref(),
        Some("https://example.com/a")
    );
    act(&mut app, Action::NextLink);
    assert_eq!(
        app.selected_text().as_deref(),
        Some("https://example.com/b")
    );
}

#[test]
fn enter_on_a_focused_link_asks_as_a_press_does_and_opens_only_on_yes() {
    use lumen_term::actions::Action;
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    act(&mut app, Action::NextLink);
    enter(&mut app);
    assert!(lines(&app).contains(URL), "the whole target is asked about");
    assert_eq!(opened.count(), 0, "nothing opens before the answer");
    assert!(
        app.focused_link().is_none(),
        "the prompt replaces the focus"
    );
    enter(&mut app);
    assert_eq!(opened.all(), [URL]);
}

#[test]
fn escape_lets_go_of_the_focus_and_another_key_does_too() {
    use lumen_term::actions::Action;
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    act(&mut app, Action::NextLink);
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.focused_link().is_none());
    assert!(!app.has_selection());
    act(&mut app, Action::NextLink);
    app.key(Key::Char('x'), lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.focused_link().is_none());
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.count(), 0);
}

#[test]
fn with_no_link_on_screen_the_keyboard_focuses_nothing() {
    use lumen_term::actions::Action;
    let (mut app, _) = start("echo plain words; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "plain words")));
    act(&mut app, Action::NextLink);
    assert!(app.focused_link().is_none());
}

const LONG: &str = "https://example.org/a/long/path/that/goes/past/the/edge";

fn start_wrapped(tail: &str) -> (App, Opened) {
    let script = format!(
        "printf '%25s' ''; printf '%s\\r\\n' '{LONG} END'; printf '%s\\r\\n' '{tail}'; read hold"
    );
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        420,
        300,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let opened = Opened::default();
    app.use_link_opener(Box::new(opened.clone()));
    assert!(pump_until(&mut app, |a| shows(a, "https://example.com/z")));
    (app, opened)
}

#[test]
fn a_wrapped_link_is_one_stop_and_is_selected_whole() {
    use lumen_term::actions::Action;
    let (mut app, _) = start_wrapped("https://example.com/z");
    let mut seen = Vec::new();
    for _ in 0..3 {
        act(&mut app, Action::NextLink);
        seen.push((
            app.focused_link().map(|h| h.target.clone()),
            app.selected_text(),
        ));
    }
    let long = Some(LONG.to_owned());
    let z = Some("https://example.com/z".to_owned());
    assert_eq!(
        seen[0],
        (long.clone(), long.clone()),
        "the whole link, not its first row"
    );
    assert_eq!(seen[1], (z.clone(), z), "the next stop is the next link");
    assert_eq!(
        seen[2],
        (long.clone(), long),
        "and it wraps back to the first"
    );
}

#[test]
fn opening_a_wrapped_link_by_keyboard_asks_with_the_whole_target() {
    use lumen_term::actions::Action;
    let (mut app, opened) = start_wrapped("https://example.com/z");
    act(&mut app, Action::NextLink);
    enter(&mut app);
    assert!(lines(&app).contains(LONG));
    enter(&mut app);
    assert_eq!(opened.all(), [LONG]);
}

#[test]
fn find_takes_enter_from_a_focused_link() {
    use lumen_term::actions::Action;
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    act(&mut app, Action::NextLink);
    act(&mut app, Action::Find);
    assert!(app.focused_link().is_none(), "find takes over the keys");
    enter(&mut app);
    assert!(
        app.link_prompt().is_none(),
        "Enter was find's, not the link's"
    );
    assert_eq!(opened.count(), 0);
}

#[test]
fn a_focused_link_the_program_replaces_is_not_asked_about() {
    use lumen_term::actions::Action;
    let dir = std::env::temp_dir().join(format!("lumen-focus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let fifo = dir.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|e| unreachable!("mkfifo: {e}"));
    assert!(made.success());
    let script = format!(
        "echo 'see https://example.com/old here'; cat '{p}' >/dev/null; printf '\\033[2J\\033[Hnothing here'; read hold",
        p = fifo.display()
    );
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    let opened = Opened::default();
    app.use_link_opener(Box::new(opened.clone()));
    assert!(pump_until(&mut app, |a| shows(a, "example.com/old")));
    act(&mut app, Action::NextLink);
    assert!(app.focused_link().is_some());
    std::fs::write(&fifo, "go").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(pump_until(&mut app, |a| shows(a, "nothing here")));
    enter(&mut app);
    assert!(
        app.link_prompt().is_none(),
        "the link is gone, so Enter asks about nothing"
    );
    assert_eq!(opened.count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keypad_enter_asks_and_then_opens_like_enter() {
    use lumen_term::actions::Action;
    let (mut app, opened) = start(SAY);
    assert!(pump_until(&mut app, |a| shows(a, "and more")));
    act(&mut app, Action::NextLink);
    let key = |app: &mut App| {
        app.key(Key::KeypadEnter, lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
    };
    key(&mut app);
    assert!(lines(&app).contains(URL));
    key(&mut app);
    assert_eq!(opened.all(), [URL]);
}

fn grid_columns(app: &App) -> usize {
    app.session()
        .document()
        .blocks()
        .iter()
        .find_map(|block| match block {
            lumen_term::Block::Grid(grid) => Some(grid.size().0),
            lumen_term::Block::Rich(_) => None,
        })
        .unwrap_or_else(|| unreachable!("a live grid"))
}

#[test]
fn identical_links_split_by_a_space_in_the_last_column_are_two_stops() {
    use lumen_term::actions::Action;
    const Z: &str = "https://example.com/z";
    let (mut probe, _) = start("echo probe; sleep 30");
    assert!(pump_until(&mut probe, |a| shows(a, "probe")));
    let columns = grid_columns(&probe);
    drop(probe);
    // The first address ends one column short of the edge, a space fills the
    // last column, and the same address starts the next row: two links.
    let pad = columns - 1 - Z.len();
    let script = format!(
        "printf '%{pad}s' ''; printf '%s' '{Z} {Z}'; printf '\\r\\n'; echo https://example.com/w; sleep 30"
    );
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |a| shows(a, "example.com/w")));
    let mut stops = Vec::new();
    for _ in 0..3 {
        act(&mut app, Action::NextLink);
        stops.push(app.selected_text());
    }
    let (z, w) = (Some(Z.to_owned()), Some("https://example.com/w".to_owned()));
    assert_eq!(
        stops,
        [z.clone(), z, w],
        "two stops for the two links, then the next"
    );
}

#[derive(Default)]
struct Recorder(std::sync::Mutex<Option<String>>);

impl lumen_term::clipboard::Clipboard for Recorder {
    fn text(&self) -> Option<String> {
        None
    }

    fn set_text(&self, text: &str) -> bool {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(text.to_owned());
        true
    }
}

impl Recorder {
    fn got(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[test]
fn the_copy_action_puts_a_wrapped_link_on_the_clipboard_with_no_line_break() {
    use lumen_term::actions::Action;
    let (mut app, _) = start_wrapped("https://example.com/z");
    act(&mut app, Action::NextLink);
    let clip = Recorder::default();
    app.perform(Action::Copy, &clip);
    assert_eq!(clip.got().as_deref(), Some(LONG));
}

#[test]
fn copy_does_not_put_an_erased_link_on_the_clipboard() {
    use lumen_term::actions::Action;
    let dir = std::env::temp_dir().join(format!("lumen-copy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let fifo = dir.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|e| unreachable!("mkfifo: {e}"));
    assert!(made.success());
    let script = format!(
        "echo 'see https://example.com/old here'; cat '{p}' >/dev/null; printf '\\033[2J\\033[Hsee https://example.com/new!'; read hold",
        p = fifo.display()
    );
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", &script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |a| shows(a, "example.com/old")));
    act(&mut app, Action::NextLink);
    std::fs::write(&fifo, "go").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(pump_until(&mut app, |a| shows(a, "example.com/new")));
    let clip = Recorder::default();
    app.perform(Action::Copy, &clip);
    assert_ne!(
        clip.got().as_deref(),
        Some("https://example.com/old"),
        "the erased link is not what the screen shows"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
