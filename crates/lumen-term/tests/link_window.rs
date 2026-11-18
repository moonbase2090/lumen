//! Links: a web address written in a program's output can be activated, the
//! terminal asks first and shows the whole target, and nothing opens until the
//! user says yes. A link a program sets with OSC 8 has no effect in a grid. A real program runs behind a real PTY; the
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

#[test]
fn a_link_a_program_sets_with_osc_8_has_no_effect_in_a_grid() {
    let (mut app, opened) = start(
        "printf '\\033]8;;https://evil.example/login\\033\\\\https://bank.example/login\\033]8;;\\033\\\\ and BETWEEN\\n'; sleep 30",
    );
    assert!(pump_until(&mut app, |a| shows(a, "BETWEEN")));
    // The text is a written address, so it is found as what it says; the
    // target the program set is not read.
    let (x, y) = pixel_of(&app, "https://bank.example/login", 3);
    assert!(app.link_press(x, y));
    assert!(lines(&app).contains("https://bank.example/login"));
    assert!(!lines(&app).contains("evil.example"));
    enter(&mut app);
    assert_eq!(opened.all(), vec!["https://bank.example/login"]);
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
fn the_pixel_wheel_is_the_prompts_too_while_it_is_up() {
    // The program asks for mouse reports and shows what it is sent, so a
    // wheel report that reached it would be on screen.
    let (mut app, _) = start(
        "printf '\\033[?1000h\\033[?1006h'; echo 'at https://example.com/w here'; stty raw -echo; cat -v",
    );
    assert!(pump_until(&mut app, |a| shows(a, "here")));
    let (x, y) = pixel_of(&app, "https://example.com/w", 2);

    // Before the prompt, a trackpad wheel reaches the program.
    assert!(
        app.wheel_pixels(x, y, 40, lumen_term::mouse::Held::default())
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    assert!(pump_until(&mut app, |a| shows(a, "^[[<64;")));

    assert!(app.link_press(x, y));
    let before = app.visible_text();
    for pixels in [40, -40, 400] {
        assert!(
            !app.wheel_pixels(x, y, pixels, lumen_term::mouse::Held::default())
                .unwrap_or_else(|e| unreachable!("{e}")),
            "the pixel wheel is the prompt's while it is up"
        );
    }
    for _ in 0..10 {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.visible_text(), before, "the program saw nothing");
    assert!(app.link_prompt().is_some());
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
    let (mut app, _) = start("echo 'at https://example.com/p here'; stty raw -echo; cat -v");
    assert!(pump_until(&mut app, |a| shows(a, "here")));
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
    for _ in 0..10 {
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!shows(&app, "second-paste"), "the program saw nothing");
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
}
