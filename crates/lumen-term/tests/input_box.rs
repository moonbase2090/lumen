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

/// Wait until `done` holds, taking the program's output as it arrives. Each
/// wait blocks on the program's output for as long as the hang bound has left;
/// nothing wakes on a clock. A `done` that depends on a tty change with no
/// output of its own is not awaited here: wait for an output marker the
/// program prints in order with that change instead.
fn until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
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
    type_str(
        &mut app,
        "sh -c 'printf \"job-%s\\n\" started; exec sleep 30'",
    );
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
    // less has drawn its first page: it holds the terminal and reads raw.
    let has_row = |a: &mut App, n: &str| grid_text(a).lines().any(|l| l.trim() == n);
    assert!(until(&mut app, |a| has_row(a, "3")), "{}", grid_text(&app));
    assert_eq!(app.input_mode(), Mode::Raw);
    type_str(&mut app, "G");
    assert!(until(&mut app, |a| has_row(a, "500")));
    type_str(&mut app, "q");
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
}

#[test]
fn a_password_read_is_hidden_and_never_drawn() {
    let mut app = zsh();
    // Echo is off (as a password prompt leaves it) before the marker is
    // printed, so the marker is ordered after the change the box reads.
    type_str(
        &mut app,
        "stty -echo; printf 'ask-%s\\n' pw; read pw; stty echo; echo got:${#pw}",
    );
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("ask-pw")));
    assert_eq!(app.input_mode(), Mode::Hidden);
    type_str(&mut app, "s3cret");
    assert_eq!(app.input_text(), "", "the secret never entered the box");
    enter(&mut app);
    // The shell read all six characters and the line, in order, so its output
    // is the signal that anything it would have echoed is already drawn.
    assert!(until(&mut app, |a| grid_text(a).contains("got:6")));
    assert!(!grid_text(&app).contains("s3cret"), "the secret was drawn");
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
    assert_eq!(app.input_text(), "", "the box handed its text over");
    assert_eq!(
        app.input_mode(),
        Mode::HandedOff,
        "zsh still holds the prefix, so the shell owns the line"
    );
    // The shell lists what completes `echo` (it holds the line, so the Ctrl+D
    // did not end it), then reads the Ctrl+C, which clears the line.
    assert!(until(&mut app, |a| grid_text(a).contains("echoti")));
    let empty_prompts = |a: &App| {
        grid_text(a)
            .lines()
            .filter(|l| l.trim_end().ends_with('%'))
            .count()
    };
    let before = empty_prompts(&app);
    app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    )
    .unwrap_or_else(|e| unreachable!("key: {e}"));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // A new empty prompt is the signal that the shell has read the Ctrl+C. It
    // drops what was typed before that, so type after it.
    assert!(until(&mut app, |a| empty_prompts(a) > before));
    // The shell is still there: it answers a line typed after the Ctrl+D.
    type_str(&mut app, "echo shell-still-here");
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a)
            .matches("shell-still-here")
            .count()
            >= 2),
        "the shell answered the line: {}",
        grid_text(&app)
    );
}

#[test]
fn a_line_typed_at_machine_speed_right_after_enter_reaches_the_shell_whole() {
    let mut app = zsh();
    for round in 0..6 {
        let word = format!("counted-{round}");
        // An empty Enter, then the next line with no pump in between: the shell
        // has not read the Enter when the first keys arrive.
        enter(&mut app);
        type_str(&mut app, &format!("echo {word}"));
        enter(&mut app);
        assert!(
            until(&mut app, |a| grid_text(a).matches(&word).count() >= 2),
            "the line ran: {}",
            grid_text(&app)
        );
        let screen = grid_text(&app);
        assert!(
            !screen.contains("command not found"),
            "the line was split between the box and the shell:\n{screen}"
        );
        assert_eq!(app.input_text(), "", "nothing was left behind in the box");
    }
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
