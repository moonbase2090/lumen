//! What a screen reader is told about the window.
//!
//! [`Snapshot`] is the facts, taken from the app and the window; [`build`] turns
//! them into an accessibility tree and nothing else, so the tree is tested
//! without a window. The window host hands the tree to the platform's adapter
//! only while an assistive technology is asking for it, so nothing is built
//! otherwise.
//!
//! The tree is a window holding the tab list (with more than one tab), a
//! terminal whose children are the rows of the screen as text and the links on
//! it, and the question about a link while one is up. The terminal is marked as
//! a polite live region. What a platform reader then announces when the rows
//! change (and in particular when the screen scrolls and rows move) has not
//! been checked, so nothing here promises that only new output is read.

use accesskit::{Action, Live, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

/// A link on the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Where it goes.
    pub target: String,
    /// The text on the screen.
    pub text: String,
}

/// The question about a link, as lines of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// What the terminal draws in the question, line by line.
    pub lines: Vec<String>,
}

/// What the window shows that a reader is told about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// The window's title.
    pub title: String,
    /// The text of each row of the screen, top down.
    pub rows: Vec<String>,
    /// The links on the screen in reading order.
    pub links: Vec<Link>,
    /// Which of `links` the keyboard has focused.
    pub focused_link: Option<usize>,
    /// The question about a link, while one is up.
    pub question: Option<Question>,
    /// The tabs' titles, and which is active.
    pub tabs: Vec<String>,
    /// The active tab.
    pub active_tab: usize,
}

const WINDOW: u64 = 1;
const TERMINAL: u64 = 2;
const TAB_LIST: u64 = 3;
const QUESTION: u64 = 4;
const OPEN: u64 = 5;
const CANCEL: u64 = 6;
const QUESTION_TEXT: u64 = 7;
const TAB: u64 = 1_000;
const ROW: u64 = 100_000;
const LINK: u64 = 200_000_000;

/// What a node stands for, from a reader's request about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The `n`th link on the screen.
    Link(usize),
    /// The Open button of the question.
    Open,
    /// The Cancel button of the question.
    Cancel,
    /// The `n`th tab.
    Tab(usize),
    /// Nothing the window acts on.
    Nothing,
}

/// What a node stands for.
#[must_use]
pub fn target_of(id: NodeId) -> Target {
    let n = id.0;
    match n {
        OPEN => Target::Open,
        CANCEL => Target::Cancel,
        _ if n >= LINK => usize::try_from(n - LINK).map_or(Target::Nothing, Target::Link),
        _ if (TAB..ROW).contains(&n) => {
            usize::try_from(n - TAB).map_or(Target::Nothing, Target::Tab)
        }
        _ => Target::Nothing,
    }
}

fn id(n: u64) -> NodeId {
    NodeId(n)
}

fn indexed(base: u64, i: usize) -> NodeId {
    id(base.saturating_add(u64::try_from(i).unwrap_or(u64::MAX / 2)))
}

/// The accessibility tree for `snapshot`: every node, with the focus on the
/// focused link, the question's Open button when it is up, or the terminal.
#[must_use]
pub fn build(snapshot: &Snapshot) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut window_children = Vec::new();

    if snapshot.tabs.len() > 1 {
        let mut tabs = Node::new(Role::TabList);
        tabs.set_label("Tabs");
        let mut kids = Vec::new();
        for (i, title) in snapshot.tabs.iter().enumerate() {
            let mut tab = Node::new(Role::Tab);
            tab.set_label(title.clone());
            tab.set_selected(i == snapshot.active_tab);
            tab.add_action(Action::Click);
            kids.push(indexed(TAB, i));
            nodes.push((indexed(TAB, i), tab));
        }
        tabs.set_children(kids);
        nodes.push((id(TAB_LIST), tabs));
        window_children.push(id(TAB_LIST));
    }

    let mut terminal = Node::new(Role::Terminal);
    terminal.set_label("Terminal");
    terminal.set_live(Live::Polite);
    let mut kids = Vec::new();
    for (i, row) in snapshot.rows.iter().enumerate() {
        let mut node = Node::new(Role::Label);
        node.set_value(row.clone());
        kids.push(indexed(ROW, i));
        nodes.push((indexed(ROW, i), node));
    }
    for (i, link) in snapshot.links.iter().enumerate() {
        let mut node = Node::new(Role::Link);
        node.set_label(link.text.clone());
        node.set_url(link.target.clone());
        node.add_action(Action::Click);
        node.add_action(Action::Focus);
        kids.push(indexed(LINK, i));
        nodes.push((indexed(LINK, i), node));
    }
    terminal.set_children(kids);
    nodes.push((id(TERMINAL), terminal));
    window_children.push(id(TERMINAL));

    let mut focus = id(TERMINAL);
    if let Some(i) = snapshot.focused_link.filter(|i| *i < snapshot.links.len()) {
        focus = indexed(LINK, i);
    }
    if let Some(question) = &snapshot.question {
        let mut dialog = Node::new(Role::Dialog);
        dialog.set_label("Open this link?");
        dialog.set_modal();
        let mut text = Node::new(Role::Label);
        text.set_value(question.lines.join("\n"));
        let mut open = Node::new(Role::Button);
        open.set_label("Open");
        open.add_action(Action::Click);
        let mut cancel = Node::new(Role::Button);
        cancel.set_label("Cancel");
        cancel.add_action(Action::Click);
        dialog.set_children(vec![id(QUESTION_TEXT), id(OPEN), id(CANCEL)]);
        nodes.push((id(QUESTION_TEXT), text));
        nodes.push((id(OPEN), open));
        nodes.push((id(CANCEL), cancel));
        nodes.push((id(QUESTION), dialog));
        window_children.push(id(QUESTION));
        focus = id(OPEN);
    }

    let mut window = Node::new(Role::Window);
    window.set_label(snapshot.title.clone());
    window.set_children(window_children);
    nodes.push((id(WINDOW), window));

    let mut info = TreeInfo::new(id(WINDOW));
    info.toolkit_name = Some("Bright".to_owned());
    TreeUpdate {
        nodes,
        tree: Some(info),
        tree_id: TreeId::ROOT,
        focus,
    }
}
