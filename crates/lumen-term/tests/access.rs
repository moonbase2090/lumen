//! What a screen reader is told: the tree the window builds from the screen,
//! and what the window does when a reader asks to act on a node. The tree is
//! built from facts read off a real [`App`] running a real program.

#![cfg(all(unix, feature = "window"))]

use accesskit::{Action, NodeId, Role, TreeUpdate};
use lumen_term::access::{Link, Question, Snapshot, Target, build, target_of};
use lumen_term::{App, Passthrough};
use std::time::{Duration, Instant};

fn node(update: &TreeUpdate, id: NodeId) -> &accesskit::Node {
    update
        .nodes
        .iter()
        .find_map(|(n, node)| (*n == id).then_some(node))
        .unwrap_or_else(|| unreachable!("node {id:?} is in the tree"))
}

fn roles(update: &TreeUpdate, role: Role) -> Vec<&accesskit::Node> {
    update
        .nodes
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.role() == role)
        .collect()
}

fn root(update: &TreeUpdate) -> NodeId {
    update
        .tree
        .as_ref()
        .map_or_else(|| unreachable!("a root"), |t| t.root)
}

fn snapshot() -> Snapshot {
    Snapshot {
        title: "Bright".to_owned(),
        rows: vec![
            "see https://example.com/a".to_owned(),
            "second row".to_owned(),
        ],
        links: vec![Link {
            target: "https://example.com/a".to_owned(),
            text: "https://example.com/a".to_owned(),
        }],
        ..Snapshot::default()
    }
}

#[test]
fn the_window_holds_a_terminal_whose_children_are_the_rows_and_the_links() {
    let update = build(&snapshot());
    let window = node(&update, root(&update));
    assert_eq!(window.role(), Role::Window);
    assert_eq!(window.label(), Some("Bright"));
    let terminals = roles(&update, Role::Terminal);
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].children().len(), 3, "two rows and one link");
    let rows: Vec<_> = roles(&update, Role::Label)
        .iter()
        .filter_map(|n| n.value())
        .collect();
    assert_eq!(rows, ["see https://example.com/a", "second row"]);
    let links = roles(&update, Role::Link);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url(), Some("https://example.com/a"));
}

#[test]
fn the_terminal_is_marked_as_a_polite_live_region() {
    let update = build(&snapshot());
    assert_eq!(
        roles(&update, Role::Terminal)[0].live(),
        Some(accesskit::Live::Polite)
    );
}

#[test]
fn the_keyboards_focused_link_is_the_readers_focus_and_otherwise_the_terminal_is() {
    let mut s = snapshot();
    let update = build(&s);
    let terminal = update
        .nodes
        .iter()
        .find_map(|(id, n)| (n.role() == Role::Terminal).then_some(*id))
        .unwrap_or_else(|| unreachable!("a terminal"));
    assert_eq!(update.focus, terminal);
    s.focused_link = Some(0);
    let update = build(&s);
    assert_eq!(target_of(update.focus), Target::Link(0));
}

#[test]
fn the_question_about_a_link_is_a_modal_dialog_with_open_and_cancel_and_takes_the_focus() {
    let mut s = snapshot();
    s.question = Some(Question {
        lines: vec![
            "Open this link?".to_owned(),
            "https://example.com/a".to_owned(),
        ],
    });
    let update = build(&s);
    let dialogs = roles(&update, Role::Dialog);
    assert_eq!(dialogs.len(), 1);
    assert!(dialogs[0].is_modal());
    let buttons: Vec<_> = roles(&update, Role::Button)
        .iter()
        .filter_map(|n| n.label())
        .collect();
    assert_eq!(buttons, ["Open", "Cancel"]);
    assert_eq!(target_of(update.focus), Target::Open);
    for button in roles(&update, Role::Button) {
        assert!(button.supports_action(Action::Click));
    }
}

#[test]
fn a_tab_list_appears_with_more_than_one_tab_and_marks_the_active_one() {
    let one = build(&snapshot());
    assert!(
        roles(&one, Role::TabList).is_empty(),
        "one tab needs no list"
    );
    let mut s = snapshot();
    s.tabs = vec!["one".to_owned(), "two".to_owned()];
    s.active_tab = 1;
    let update = build(&s);
    assert_eq!(roles(&update, Role::TabList).len(), 1);
    let tabs = roles(&update, Role::Tab);
    assert_eq!(tabs.len(), 2);
    assert_eq!(
        tabs.iter().map(|t| t.is_selected()).collect::<Vec<_>>(),
        [Some(false), Some(true)]
    );
}

fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
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

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        800,
        480,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

fn shows(app: &App, text: &str) -> bool {
    app.visible_text().iter().any(|r| r.contains(text))
}

fn targets(app: &App) -> Vec<String> {
    app.view_links()
        .iter()
        .map(|l| l.target().to_owned())
        .collect()
}

#[test]
fn a_reader_acting_on_a_link_node_asks_the_same_question_a_press_asks_and_opens_nothing() {
    let mut app = start("echo 'see https://example.com/a and https://example.com/b'; sleep 30");
    assert!(pump_until(&mut app, |a| shows(a, "example.com/b")));
    assert_eq!(
        targets(&app),
        ["https://example.com/a", "https://example.com/b"]
    );
    assert!(app.focus_view_link(1));
    assert_eq!(
        app.view_links().iter().position(|l| l.focused),
        Some(1),
        "the keyboard's focus is the link the reader focused"
    );
    assert!(app.ask_about_view_link(0));
    let question = app
        .link_prompt()
        .unwrap_or_else(|| unreachable!("the question is up"));
    assert!(
        question
            .lines()
            .join("\n")
            .contains("https://example.com/a")
    );
    assert!(!app.ask_about_view_link(9), "no such link");
}

#[test]
fn the_links_a_reader_gets_are_those_of_the_rows_it_gets_when_the_view_is_scrolled_back() {
    // An old address scrolls off into the history; a new one is on the live
    // screen. With the view scrolled back to the old one, the rows show the old
    // address, so the links, and what acting on one asks about, are the old one.
    let script = "echo 'old https://example.com/old'; i=0; while [ $i -lt 120 ]; do echo filler $i; i=$((i+1)); done; echo 'new https://example.com/new'; sleep 30";
    let mut app = start(script);
    assert!(pump_until(&mut app, |a| shows(a, "example.com/new")));
    assert_eq!(
        targets(&app),
        ["https://example.com/new"],
        "following: the live screen"
    );
    let mut tries = 0;
    while !shows(&app, "example.com/old") && tries < 200 {
        app.key(lumen_term::Key::PageUp, lumen_term::Mods::default())
            .unwrap_or_else(|e| unreachable!("{e}"));
        tries += 1;
    }
    assert!(
        shows(&app, "example.com/old"),
        "scrolled back to the old address"
    );
    assert!(!shows(&app, "example.com/new"));
    assert_eq!(
        targets(&app),
        ["https://example.com/old"],
        "the links are the view's"
    );
    assert!(app.ask_about_view_link(0));
    let question = app
        .link_prompt()
        .unwrap_or_else(|| unreachable!("the question is up"));
    let text = question.lines().join("\n");
    assert!(text.contains("https://example.com/old"), "{text}");
    assert!(!text.contains("example.com/new"), "{text}");
    assert!(
        !app.focus_view_link(0),
        "a link in the scrollback is not a keyboard stop"
    );
}

fn columns_of(app: &App) -> usize {
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

/// An app on a program that has printed `printf_args` and then waits, with the
/// live grid's width, once `marker` is on the screen.
fn started_with(script_for: impl Fn(usize) -> String, marker: &str) -> App {
    let mut probe = start("echo probe; sleep 30");
    assert!(pump_until(&mut probe, |a| shows(a, "probe")));
    let columns = columns_of(&probe);
    drop(probe);
    let mut app = start(&script_for(columns));
    assert!(pump_until(&mut app, |a| shows(a, marker)));
    app
}

#[test]
fn every_row_of_text_with_an_address_has_its_link_at_every_pixel_of_scroll() {
    // 150 rows, each with its own address, so most are in the history.
    let script = "i=0; while [ $i -lt 150 ]; do echo \"row $i https://example.com/r$i\"; i=$((i+1)); done; echo end-marker; sleep 30";
    let mut app = start(script);
    assert!(pump_until(&mut app, |a| shows(a, "end-marker")));
    let ch = i64::from(app.cell_height());
    app.scroll_end(true);
    for step in 0..(2 * ch) {
        let rows = app.visible_text();
        let links = app.view_links();
        for (i, row) in rows.iter().enumerate() {
            if let Some(at) = row.find("https://example.com/") {
                let want = &row[at..];
                assert!(
                    links.iter().any(|l| l.view_row == i && l.target() == want),
                    "pixel {step}: row {i} ({row:?}) has no link; links: {:?}",
                    links
                        .iter()
                        .map(|l| (l.view_row, l.target().to_owned()))
                        .collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(
            links.len(),
            rows.iter()
                .filter(|r| r.contains("https://example.com/"))
                .count(),
            "pixel {step}: one link for each row with an address"
        );
        assert!(app.scroll_pixels(-1), "the view moves down a pixel");
    }
}

#[test]
fn an_address_over_three_rows_is_one_link_that_can_be_focused() {
    let mut app = started_with(
        |columns| {
            let url = format!("https://example.com/{}", "a".repeat(columns * 2 + 10));
            format!("echo '{url}'; echo tail-marker; sleep 30")
        },
        "tail-marker",
    );
    let links = app.view_links();
    assert_eq!(links.len(), 1, "one address over three rows is one link");
    assert!(links[0].target().starts_with("https://example.com/aaa"));
    assert!(app.focus_view_link(0), "a wrapped link takes focus");
    assert_eq!(app.view_links().iter().position(|l| l.focused), Some(0));
}

#[test]
fn an_address_over_two_rows_takes_focus_from_a_reader() {
    let mut app = started_with(
        |columns| {
            let url = format!("https://example.com/{}", "b".repeat(columns + 5));
            format!("echo 'xx {url}'; echo tail-marker; sleep 30")
        },
        "tail-marker",
    );
    assert_eq!(app.view_links().len(), 1);
    assert!(app.focus_view_link(0));
    assert_eq!(app.view_links().iter().position(|l| l.focused), Some(0));
}

#[test]
fn identical_addresses_across_a_hard_newline_are_two_links() {
    let mut app = started_with(
        |columns| {
            // Each address fills a whole row, the first ends in the last column,
            // and a real line break (not a wrap) comes before the second.
            let url = format!("https://example.com/{}", "c".repeat(columns - 20));
            format!("echo '{url}'; echo '{url}'; echo tail-marker; sleep 30")
        },
        "tail-marker",
    );
    let links = app.view_links();
    assert_eq!(
        links.len(),
        2,
        "{:?}",
        links.iter().map(|l| l.target().len()).collect::<Vec<_>>()
    );
    assert_eq!(links[0].target(), links[1].target());
    let _ = &mut app;
}
