//! The input box against real programs on a real PTY: at a zsh prompt typing
//! goes into the box and Enter runs the line; vim and less get raw keys; a
//! password read shows a status and never draws what is typed.

#![cfg(unix)]

use lumen_term::actions::Action;
use lumen_term::clipboard::Clipboard;
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
    // The marker comes from the job itself, so it holds the terminal before
    // Ctrl-C is sent (the box reads Raw earlier too, while the shell is
    // still starting the job on a cooked tty).
    type_str(&mut app, "sh -c 'echo job-started; exec sleep 30'");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("job-started")));
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

struct Fixed(&'static str);

impl Clipboard for Fixed {
    fn text(&self) -> Option<String> {
        Some(self.0.to_owned())
    }
}

#[test]
fn the_paste_chord_lands_in_the_box_and_nothing_runs() {
    let mut app = zsh();
    type_str(&mut app, "echo ");
    app.perform(Action::Paste, &Fixed("one\ntwo"));
    assert_eq!(app.input_text(), "echo one\ntwo");
    // Empty the box and run a marker. The shell reads what it was sent in
    // order, so once the marker's output is on screen anything the paste had
    // sent would have run before it: no sleep, and the shell's redraw when the
    // box grows and shrinks does not matter.
    for _ in 0.."echo one\ntwo".chars().count() {
        app.key(Key::Backspace, Mods::default())
            .unwrap_or_else(|e| unreachable!("backspace: {e}"));
    }
    assert_eq!(app.input_text(), "");
    type_str(&mut app, "echo marker-after-paste");
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a)
            .matches("marker-after-paste")
            .count()
            >= 2),
        "the marker ran: {}",
        grid_text(&app)
    );
    let screen = grid_text(&app);
    assert!(
        !screen.contains("one") && !screen.contains("two"),
        "the shell was sent nothing from the paste:\n{screen}"
    );
}

#[test]
fn control_d_after_box_text_leaves_the_shell_holding_the_line() {
    let mut app = zsh();
    type_str(&mut app, "echo");
    app.key(
        Key::Char('d'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    std::thread::sleep(Duration::from_millis(400));
    app.pump();
    assert_eq!(app.input_text(), "", "the box handed its text over");
    assert_eq!(
        app.input_mode(),
        Mode::HandedOff,
        "zsh still holds the prefix, so the shell owns the line"
    );
    // The line is finished by the shell: Ctrl+C clears it and the box is back.
    app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
}

/// The prompts the shell has drawn: `zsh -f` ends its prompt with `%`, or
/// with `#` for root.
fn prompts(app: &App) -> usize {
    grid_text(app)
        .lines()
        .filter(|l| l.trim_end().ends_with(['%', '#']) || l.contains("% ") || l.contains("# "))
        .count()
}

/// Press Enter and wait for the shell's next prompt, so that no key is typed
/// while the shell is between prompts.
fn enter_and_wait(app: &mut App) {
    let before = prompts(app);
    enter(app);
    assert!(
        until(app, |a| prompts(a) > before
            && a.input_mode() == Mode::Editing),
        "the shell drew its next prompt: {}",
        grid_text(app)
    );
}

#[test]
fn every_enter_the_shell_takes_is_counted_and_one_a_program_takes_is_not() {
    let mut app = zsh();
    // The box can take keys before the first prompt is drawn.
    assert!(until(&mut app, |a| prompts(a) >= 1));
    assert_eq!(app.forwarded_enters(), 0);
    type_str(&mut app, "echo counted-one");
    enter_and_wait(&mut app);
    assert!(
        grid_text(&app).contains("counted-one\n"),
        "{}",
        grid_text(&app)
    );
    assert_eq!(app.forwarded_enters(), 1, "the box's line");
    enter_and_wait(&mut app);
    assert_eq!(app.forwarded_enters(), 2, "an empty Enter");
    // cat is in front: its Enters are its own.
    type_str(&mut app, "cat");
    enter(&mut app);
    assert!(
        until(&mut app, |a| a.session().tty_state().shell_in_front
            == Some(false)
            && a.input_mode() == Mode::Raw),
        "cat is in front: {}",
        grid_text(&app)
    );
    assert_eq!(app.forwarded_enters(), 3);
    type_str(&mut app, "to-cat");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).matches("to-cat").count() >= 2));
    assert_eq!(
        app.forwarded_enters(),
        3,
        "an Enter cat read is not counted"
    );
    app.key(
        Key::Char('d'),
        Mods {
            ctrl: true,
            alt: false,
            shift: false,
        },
    )
    .unwrap_or_else(|e| unreachable!("ctrl-d: {e}"));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    assert_eq!(app.forwarded_enters(), 3, "Ctrl+D is not an Enter");
}
