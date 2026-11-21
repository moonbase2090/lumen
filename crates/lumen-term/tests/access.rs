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
fn the_terminal_is_a_polite_live_region_so_new_output_is_read_without_the_whole_screen() {
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

#[test]
fn a_reader_acting_on_a_link_node_asks_the_same_question_a_press_asks_and_opens_nothing() {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &[
            "-c",
            "echo 'see https://example.com/a and https://example.com/b'; sleep 30",
        ],
        1,
        800,
        480,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
    assert!(pump_until(&mut app, |a| a
        .visible_text()
        .iter()
        .any(|r| r.contains("example.com/b"))));
    let links = app.screen_links();
    assert_eq!(
        links.iter().map(|l| l.target.as_str()).collect::<Vec<_>>(),
        ["https://example.com/a", "https://example.com/b"]
    );
    assert!(app.focus_link_place(1));
    assert_eq!(app.focused_link_place(), Some(1));
    assert!(app.ask_about_link_place(0));
    let question = app
        .link_prompt()
        .unwrap_or_else(|| unreachable!("the question is up"));
    assert!(
        question
            .lines()
            .join("\n")
            .contains("https://example.com/a")
    );
    assert!(!app.ask_about_link_place(9), "no such link");
}
