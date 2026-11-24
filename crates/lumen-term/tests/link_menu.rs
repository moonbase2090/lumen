//! Right-clicking a link: the menu adds Open Link and Copy Address. Open Link
//! raises the terminal's own question and never opens by itself. Copy Address
//! raises the same question for a copy and puts only the checked target on the
//! clipboard, never the text the link shows. A `file` link and a refused target
//! are inert. A real program runs behind a real PTY; the system opener and the
//! clipboard are replaced by ones that record what they were given.

#![cfg(all(unix, feature = "links-clipboard"))]

use lumen_term::actions::Action;
use lumen_term::app::Outcome;
use lumen_term::clipboard::Clipboard;
use lumen_term::contextmenu::LinkItems;
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
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A clipboard that records every text put on it, and can refuse writes.
#[derive(Clone, Default)]
struct Copied {
    writes: Arc<Mutex<Vec<String>>>,
    refuse: bool,
}

impl Clipboard for Copied {
    fn text(&self) -> Option<String> {
        None
    }

    fn set_text(&self, text: &str) -> bool {
        if self.refuse {
            return false;
        }
        self.writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(text.to_owned());
        true
    }
}

impl Copied {
    fn all(&self) -> Vec<String> {
        self.writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A clipboard for the menu's own Copy and Paste, which these tests never use.
struct Unused;

impl Clipboard for Unused {
    fn text(&self) -> Option<String> {
        None
    }
}

fn start_with(script: &str, copied: &Copied) -> (App, Opened) {
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
    app.use_link_clipboard(Box::new(copied.clone()));
    (app, opened)
}

fn start(script: &str) -> (App, Opened, Copied) {
    let copied = Copied::default();
    let (app, opened) = start_with(script, &copied);
    (app, opened, copied)
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

/// A pixel over the link whose target is `target`.
fn pixel_of_link(app: &App, target: &str) -> (u32, u32) {
    (0..SIZE.1)
        .step_by(2)
        .flat_map(|y| (0..SIZE.0).step_by(2).map(move |x| (x, y)))
        .find(|&(x, y)| app.link_at(x, y).is_some_and(|h| h.target == target))
        .unwrap_or_else(|| unreachable!("no link to {target} in view"))
}

/// A pixel on a row that is not a link.
fn pixel_off_links(app: &App) -> (u32, u32) {
    (0..SIZE.1)
        .step_by(2)
        .flat_map(|y| (0..SIZE.0).step_by(2).map(move |x| (x, y)))
        .find(|&(x, y)| app.live_cell(x, y).is_some() && app.link_at(x, y).is_none())
        .unwrap_or_else(|| unreachable!("every pixel is a link"))
}

/// A shell command that prints `text` as an OSC 8 link to `target`, closed,
/// then `AFTER`, and waits.
fn osc8(target: &str, text: &str) -> String {
    format!("printf '\\033]8;;{target}\\033\\\\{text}\\033]8;;\\033\\\\ AFTER\\n'; sleep 30")
}

/// Nothing recorded.
fn no_text() -> Vec<String> {
    Vec::new()
}

fn lines(app: &App) -> String {
    app.link_prompt()
        .map(|p| p.lines().join("\n"))
        .unwrap_or_default()
}

fn enter(app: &mut App) {
    app.key(Key::Enter, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn escape(app: &mut App) {
    app.key(Key::Escape, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
}

/// What the menu opened at `at` offers, as (action, on).
fn offered(app: &App) -> Vec<(Action, bool)> {
    app.context_menu()
        .unwrap_or_else(|| unreachable!("a menu is open"))
        .items()
        .iter()
        .flatten()
        .map(|(action, _, on)| (*action, *on))
        .collect()
}

#[test]
fn a_right_click_on_a_web_link_adds_open_link_and_copy_address_turned_on() {
    let (mut app, _, _) = start(&osc8("https://real.example/docs", "read the docs"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/docs");
    assert!(app.open_context_menu(x, y, true));
    let items = offered(&app);
    assert_eq!(
        items.iter().take(2).copied().collect::<Vec<_>>(),
        vec![(Action::OpenLink, true), (Action::CopyLinkAddress, true)]
    );
    assert!(
        items.contains(&(Action::Copy, false)),
        "Copy is still its own item"
    );
}

#[test]
fn a_right_click_off_a_link_has_no_link_items_and_forgets_the_last_link() {
    let (mut app, opened, copied) = start(&osc8("https://real.example/docs", "go"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/docs");
    assert!(app.open_context_menu(x, y, true));
    let (ox, oy) = pixel_off_links(&app);
    assert!(app.open_context_menu(ox, oy, true), "the menu moves");
    let items = offered(&app);
    assert!(
        !items
            .iter()
            .any(|(a, _)| matches!(a, Action::OpenLink | Action::CopyLinkAddress)),
        "{items:?}"
    );
    // A link item chosen after that right click has no link to act on.
    assert_eq!(app.perform(Action::OpenLink, &Unused), Outcome::Nothing);
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Nothing
    );
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.all(), no_text());
    assert_eq!(copied.all(), no_text());
}

#[test]
fn open_link_asks_first_and_opens_only_on_yes() {
    let (mut app, opened, _) = start(&osc8("https://real.example/docs?a=1", "read the docs"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/docs?a=1");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(app.perform(Action::OpenLink, &Unused), Outcome::Redraw);
    let prompt = lines(&app);
    assert!(prompt.contains("Open this link?"), "{prompt}");
    assert!(prompt.contains("https://real.example/docs?a=1"), "{prompt}");
    assert!(prompt.contains("Text shown: read the docs"), "{prompt}");
    assert_eq!(opened.all(), no_text(), "choosing the item opens nothing");
    enter(&mut app);
    assert_eq!(opened.all(), vec!["https://real.example/docs?a=1"]);
}

#[test]
fn cancelling_open_link_opens_nothing() {
    let (mut app, opened, _) = start(&osc8("https://real.example/", "go"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(app.perform(Action::OpenLink, &Unused), Outcome::Redraw);
    escape(&mut app);
    assert!(app.link_prompt().is_none());
    assert_eq!(opened.all(), no_text());
}

#[test]
fn copy_address_of_a_lookalike_copies_the_checked_target_not_the_text_shown() {
    let (mut app, opened, copied) = start(&osc8(
        "https://evil.example/login",
        "https://bank.example/login",
    ));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://evil.example/login");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Redraw
    );
    let prompt = lines(&app);
    assert!(prompt.contains("Copy this address?"), "{prompt}");
    assert!(prompt.contains("https://evil.example/login"), "{prompt}");
    assert!(
        prompt.contains("Warning: the text shown looks like"),
        "{prompt}"
    );
    assert!(
        copied.all().is_empty(),
        "nothing is copied before the answer"
    );
    enter(&mut app);
    assert_eq!(copied.all(), vec!["https://evil.example/login"]);
    assert!(
        !copied.all().iter().any(|t| t.contains("bank.example")),
        "the text the link showed is never what is copied"
    );
    assert_eq!(opened.all(), no_text(), "copying opens nothing");
}

#[test]
fn cancelling_copy_address_copies_nothing() {
    let (mut app, _, copied) = start(&osc8("https://real.example/", "go"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Redraw
    );
    escape(&mut app);
    assert!(app.link_prompt().is_none());
    assert_eq!(copied.all(), no_text());
}

#[test]
fn copy_address_of_a_mail_link_copies_the_checked_form_without_the_dropped_fields() {
    let target = "mailto:a@lumenprotocol.org?bcc=hidden@lumenprotocol.org&subject=hi";
    let (mut app, _, copied) = start(&osc8(target, "write to us"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, target);
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Redraw
    );
    enter(&mut app);
    assert_eq!(copied.all(), vec!["mailto:a@lumenprotocol.org?subject=hi"]);
}

#[test]
fn a_file_link_and_a_refused_scheme_are_inert_in_the_menu_and_when_forced() {
    for (target, text) in [
        ("file:///etc/hosts", "hosts"),
        ("javascript:alert(1)", "click me"),
        ("ftp://example.com/x", "files"),
    ] {
        let (mut app, opened, copied) = start(&osc8(target, text));
        assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
        let (x, y) = pixel_of_link(&app, target);
        assert!(app.open_context_menu(x, y, true));
        let items = offered(&app);
        assert_eq!(
            items.iter().take(2).copied().collect::<Vec<_>>(),
            vec![(Action::OpenLink, false), (Action::CopyLinkAddress, false)],
            "{target} is shown turned off"
        );
        // Even if an item were chosen anyway, the question only informs.
        for action in [Action::OpenLink, Action::CopyLinkAddress] {
            assert!(app.open_context_menu(x, y, true));
            assert_eq!(app.perform(action, &Unused), Outcome::Redraw);
            enter(&mut app);
            assert!(app.link_prompt().is_none());
        }
        assert_eq!(opened.all(), no_text(), "{target} was opened");
        assert_eq!(copied.all(), no_text(), "{target} was copied");
    }
}

#[test]
fn a_copy_the_clipboard_refuses_is_reported_and_copies_nothing() {
    let copied = Copied {
        refuse: true,
        ..Copied::default()
    };
    let (mut app, _) = start_with(&osc8("https://real.example/", "go"), &copied);
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Redraw
    );
    assert!(app.answer_link(true).is_err());
    assert_eq!(copied.all(), no_text());
}

#[test]
fn a_link_the_program_wrote_as_plain_text_gets_the_items_too() {
    let (mut app, opened, copied) =
        start("echo 'docs at https://example.com/guide?a=1 end'; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "end")));
    let (x, y) = pixel_of_link(&app, "https://example.com/guide?a=1");
    assert!(app.open_context_menu(x, y, true));
    assert_eq!(
        offered(&app).first().copied(),
        Some((Action::OpenLink, true))
    );
    assert_eq!(
        app.perform(Action::CopyLinkAddress, &Unused),
        Outcome::Redraw
    );
    enter(&mut app);
    assert_eq!(copied.all(), vec!["https://example.com/guide?a=1"]);
    assert_eq!(opened.all(), no_text());
}

#[test]
fn the_state_a_system_menu_is_built_from_follows_the_pointer() {
    let (mut app, _, _) = start(&osc8("https://real.example/", "go"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/");
    assert_eq!(app.context_state(x, y, true).link, LinkItems::On);
    let (ox, oy) = pixel_off_links(&app);
    assert_eq!(app.context_state(ox, oy, true).link, LinkItems::Absent);
}

/// Where the terminal draws the menu itself (Linux and the other Unix
/// desktops), a right click on a link adds the link items at the top and
/// Settings stays the last entry, and the keys reach both ends.
#[cfg(not(target_os = "macos"))]
#[test]
fn over_a_link_the_drawn_menu_keeps_settings_last_and_the_keys_reach_it() {
    let (mut app, opened, copied) = start(&osc8("https://real.example/docs", "read the docs"));
    assert!(pump_until(&mut app, |a| shows(a, "AFTER")));
    let (x, y) = pixel_of_link(&app, "https://real.example/docs");
    assert!(app.open_context_menu(x, y, true));
    let entries: Vec<Option<Action>> = app
        .context_menu()
        .unwrap_or_else(|| unreachable!("a menu is open"))
        .items()
        .iter()
        .map(|entry| entry.map(|(action, _, _)| action))
        .collect();
    assert_eq!(
        entries,
        vec![
            Some(Action::OpenLink),
            Some(Action::CopyLinkAddress),
            None,
            Some(Action::Copy),
            Some(Action::Paste),
            Some(Action::SelectAll),
            None,
            Some(Action::ZoomIn),
            Some(Action::ZoomOut),
            Some(Action::ZoomReset),
            None,
            Some(Action::Settings),
        ]
    );
    let chosen = |app: &App| {
        app.context_menu()
            .and_then(lumen_term::contextmenu::ContextMenu::chosen)
    };
    let up = |app: &mut App| {
        app.key(Key::Up, lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
    };
    // Up from nothing highlighted lands on the last entry, Settings; Down
    // from there wraps to the first, Open Link.
    up(&mut app);
    assert_eq!(chosen(&app), Some(Action::Settings));
    app.key(Key::Down, lumen_term::Mods::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(chosen(&app), Some(Action::OpenLink));
    // Back up to Settings, and Enter chooses it: the window then opens the file.
    up(&mut app);
    enter(&mut app);
    assert_eq!(app.take_menu_choice(), Some(Action::Settings));
    assert!(app.context_menu().is_none());
    assert_eq!(opened.all(), no_text(), "the link was not opened");
    assert_eq!(copied.all(), no_text(), "nothing was copied");
}
