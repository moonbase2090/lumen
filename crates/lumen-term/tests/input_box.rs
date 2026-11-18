//! The input box against real programs on a real PTY: at a zsh prompt typing
//! goes into the box and Enter runs the line; vim and less get raw keys; a
//! password read shows a status and never draws what is typed.

#![cfg(unix)]

use lumen_term::input_line::Mode;
use lumen_term::{App, Block, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

fn start(program: &str, args: &[&str]) -> App {
    App::start(Box::new(Passthrough), program, args, 1, 800, 600)
        .unwrap_or_else(|e| unreachable!("{program} starts: {e}"))
}

fn grid_text(app: &App) -> String {
    app.session()
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
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

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.key(Key::Char(c), Mods::default())
            .unwrap_or_else(|e| unreachable!("key: {e}"));
    }
}

fn enter(app: &mut App) {
    app.key(Key::Enter, Mods::default())
        .unwrap_or_else(|e| unreachable!("enter: {e}"));
}

fn zsh() -> App {
    let mut app = start("/bin/zsh", &["-f", "-i"]);
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "zsh reaches its prompt and the box takes the keys"
    );
    app
}

#[test]
fn at_a_zsh_prompt_typing_goes_into_the_box_and_enter_runs_it() {
    let mut app = zsh();
    type_str(&mut app, "echo hi-from-box");
    assert_eq!(app.input_text(), "echo hi-from-box");
    assert!(
        !grid_text(&app).contains("hi-from-box"),
        "nothing reached the shell before Enter"
    );
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("hi-from-box")));
    assert_eq!(app.input_text(), "", "the box is empty after the line ran");
}

#[test]
fn a_control_key_is_never_swallowed_by_the_box() {
    let mut app = zsh();
    type_str(&mut app, "sleep 30");
    enter(&mut app);
    assert!(until(&mut app, |a| a.input_mode() == Mode::Raw));
    app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "Ctrl-C reached sleep, which ended, and the prompt is back"
    );
}

#[test]
fn vim_gets_raw_keys_and_the_box_stays_empty() {
    let mut app = zsh();
    type_str(&mut app, "vim -u NONE -N");
    enter(&mut app);
    assert!(until(&mut app, |a| a.input_mode() == Mode::Raw));
    type_str(&mut app, "ihello-vim");
    assert!(until(&mut app, |a| grid_text(a).contains("hello-vim")));
    assert_eq!(app.input_text(), "");
}

#[test]
fn less_gets_raw_keys() {
    let mut app = zsh();
    type_str(&mut app, "seq 1 500 | less");
    enter(&mut app);
    assert!(until(&mut app, |a| a.input_mode() == Mode::Raw));
    type_str(&mut app, "G");
    assert!(until(&mut app, |a| grid_text(a).contains("500")));
    type_str(&mut app, "q");
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
}

#[test]
fn a_password_read_is_hidden_and_never_drawn() {
    let mut app = zsh();
    type_str(&mut app, "read -s pw; echo got:${#pw}");
    enter(&mut app);
    assert!(until(&mut app, |a| a.input_mode() == Mode::Hidden));
    type_str(&mut app, "s3cret");
    std::thread::sleep(Duration::from_millis(200));
    app.pump();
    assert!(!grid_text(&app).contains("s3cret"));
    assert_eq!(app.input_text(), "", "the secret never entered the box");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("got:6")));
}
