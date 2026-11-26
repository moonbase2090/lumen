//! The `input_box` setting against a real zsh on a real PTY. With the box off,
//! keys at the prompt go to the shell as typed and the program gets the rows
//! the box took; a line already in the box when it is turned off is kept and
//! sent only by its Enter; turned back on, the box takes the keys again.

#![cfg(unix)]

use lumen_term::app::EDITOR_ROWS;
use lumen_term::input_line::Mode;
use lumen_term::{App, Block, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

fn zsh() -> App {
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/zsh",
        &["-f", "-i"],
        1,
        800,
        600,
    )
    .unwrap_or_else(|e| unreachable!("zsh starts: {e}"));
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "zsh reaches its prompt and the box takes the keys"
    );
    app
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

/// The rows of the live grid: what the program was told its window is.
fn grid_rows(app: &App) -> usize {
    match app.session().document().blocks().last() {
        Some(Block::Grid(g)) => g.size().1,
        _ => 0,
    }
}

/// Wait until `done` holds, taking the program's output as it arrives; each
/// wait blocks on output, nothing wakes on a clock.
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

#[test]
fn with_the_box_off_keys_reach_the_shell_as_typed_and_the_program_gets_the_rows() {
    let mut app = zsh();
    // The box can take the keys from the tty state alone, before the prompt's
    // output has made a grid: wait for the grid before reading its rows.
    assert!(
        until(&mut app, |a| grid_rows(a) > 0),
        "the shell's output makes a grid"
    );
    let with_box = grid_rows(&app);
    assert!(app.set_input_box(false), "the window draws again");
    assert_eq!(app.input_mode(), Mode::Raw, "the box takes no keys");
    assert!(
        until(&mut app, |a| grid_rows(a)
            == with_box + EDITOR_ROWS as usize),
        "the program is told it has the box's rows: {} then {}",
        with_box,
        grid_rows(&app)
    );

    type_str(&mut app, "echo inline-$((40+2))");
    assert_eq!(app.input_text(), "", "nothing waits in a box");
    assert!(
        until(&mut app, |a| grid_text(a).contains("echo inline-$((40+2))")),
        "the shell has the keys before Enter and echoes them on its prompt line"
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("inline-42")),
        "Enter runs the line the shell holds"
    );
    assert_eq!(app.input_mode(), Mode::Raw);
}

#[test]
fn turning_the_box_off_keeps_a_line_already_in_it_until_its_enter() {
    let mut app = zsh();
    type_str(&mut app, "echo kept-$((1+1))");
    app.set_input_box(false);
    assert_eq!(app.input_text(), "echo kept-$((1+1))", "the line is kept");
    assert_eq!(app.input_mode(), Mode::Editing, "the box still edits it");
    type_str(&mut app, "3");
    app.pump();
    assert!(
        !grid_text(&app).contains("kept"),
        "nothing reached the shell before the box's Enter"
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("kept-23")),
        "the box's Enter sends the line whole"
    );
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Raw),
        "at the next prompt the box is off"
    );
}

#[test]
fn turned_back_on_the_box_takes_the_keys_at_the_next_prompt() {
    let mut app = zsh();
    app.set_input_box(false);
    // A program the shell starts, not a builtin: the tty shows the shell leave
    // its prompt and come back, which is what tells the box the line was read.
    type_str(&mut app, "/bin/echo off-$((2+2))");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("off-4")));
    assert!(app.set_input_box(true), "the box comes back");
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "the box takes the keys once the shell is at its prompt again"
    );
    type_str(&mut app, "echo on-$((3+3))");
    assert_eq!(app.input_text(), "echo on-$((3+3))");
    app.pump();
    assert!(
        !grid_text(&app).contains("echo on-"),
        "nothing sent before Enter"
    );
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("on-6")));
}

/// The pixel rows of the gutter that carry ink: the rows a gutter mark is on.
/// The gutter's own colour on each row is read at its left edge, which no mark
/// reaches; the divider at its right edge is left out.
fn gutter_ink_rows(frame: &lumen_term::Frame, cell_w: u32) -> Vec<u32> {
    let inside = 2 * cell_w - 2;
    (0..frame.height)
        .filter(|&y| {
            let plain = frame.pixel(1, y);
            (2..inside).any(|x| frame.pixel(x, y) != plain)
        })
        .collect()
}

/// Write `frame` to the capture directory, when the run names one.
fn capture(frame: &lumen_term::Frame, name: &str) {
    let Some(dir) = std::env::var_os("LUMEN_CAPTURE_DIR") else {
        return;
    };
    let png = frame
        .to_png()
        .unwrap_or_else(|e| unreachable!("the frame encodes: {e}"));
    let path = std::path::Path::new(&dir).join(name);
    assert!(std::fs::write(&path, png).is_ok(), "{}", path.display());
}

fn no_gutter_ink(app: &App) -> bool {
    gutter_ink_rows(&app.view(), app.cell_width()).is_empty()
}

/// The kernel facts alone say the shell is at a prompt: it is in front and
/// reading without ICANON. On a layer without sealing keys that is all there
/// is to go on.
fn kernel_says_prompt(app: &App) -> bool {
    let tty = app.session().tty_state();
    tty.shell_in_front == Some(true) && tty.icanon == Some(false)
}

// These shells run on a layer without sealing keys, as every shell does on
// macOS. There the prompt mark is never drawn: no boundary there is one a
// program cannot print, and the kernel facts read the same for a builtin that
// reads keys behind a printed prompt. The sealed tests cover where it is drawn.

#[test]
fn without_sealing_keys_the_box_off_draws_no_prompt_mark() {
    let mut app = zsh();
    assert!(no_gutter_ink(&app), "with the box on the gutter is empty");
    capture(&app.view(), "box-prompt.png");
    app.set_input_box(false);
    assert!(until(&mut app, |a| kernel_says_prompt(a)));
    assert!(no_gutter_ink(&app), "no mark without a sealed prompt");
    capture(&app.view(), "inline-prompt.png");
    type_str(&mut app, "echo typed");
    assert!(until(&mut app, |a| grid_text(a).contains("echo typed")));
    assert!(no_gutter_ink(&app));
}

#[test]
fn a_builtin_reading_keys_behind_a_printed_prompt_gets_no_mark() {
    let mut app = zsh();
    app.set_input_box(false);
    // stty puts the tty in the state `read -k` reads in before the prompt is
    // printed, so while it waits the shell is in front with ICANON off: to the
    // kernel facts this is the shell's prompt.
    type_str(
        &mut app,
        "stty -icanon -echo; printf 'fa''ke-builtin %% '; read -k k; stty sane; echo got-$k-done",
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("fake-builtin %")),
        "the builtin printed its prompt"
    );
    assert!(
        kernel_says_prompt(&app),
        "the kernel facts read as a prompt"
    );
    assert!(no_gutter_ink(&app), "no mark for a builtin reading keys");
    capture(&app.view(), "inline-fake-builtin.png");
    type_str(&mut app, "q");
    assert!(
        until(&mut app, |a| grid_text(a).contains("got-q-done")),
        "the key reached read -k"
    );
}

#[test]
fn a_program_that_prints_a_prompt_of_its_own_gets_no_mark() {
    let mut app = zsh();
    app.set_input_box(false);
    // A program in front that reads a cooked line after printing a prompt
    // that looks like the shell's.
    type_str(
        &mut app,
        "sh -c 'printf \"fa\"\"ke-cooked %% \"; read x; echo co\"\"oked-done'",
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("fake-cooked %")),
        "the program printed its prompt"
    );
    assert!(no_gutter_ink(&app), "no mark for a program's prompt");
    capture(&app.view(), "inline-fake-cooked.png");
    type_str(&mut app, "x");
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("cooked-done")));

    // One that reads keys raw, as a line editor does, behind the same kind of
    // prompt and with the cursor where the shell's would be.
    type_str(
        &mut app,
        "sh -c 'stty -icanon -echo; printf \"fa\"\"ke-raw %% \"; dd bs=1 count=1 >/dev/null 2>&1; stty sane; echo r\"\"aw-done'",
    );
    enter(&mut app);
    assert!(
        until(&mut app, |a| grid_text(a).contains("fake-raw %")),
        "the program printed its prompt"
    );
    assert!(
        no_gutter_ink(&app),
        "no mark for a program reading raw keys"
    );
    capture(&app.view(), "inline-fake-raw.png");
    type_str(&mut app, "q");
    assert!(until(&mut app, |a| grid_text(a).contains("raw-done")));
}

#[test]
fn with_the_box_off_a_full_screen_program_gets_raw_keys_and_no_mark() {
    let mut app = zsh();
    app.set_input_box(false);
    type_str(&mut app, "vim -u NONE -N");
    enter(&mut app);
    type_str(&mut app, "ihello-vim");
    assert!(until(&mut app, |a| grid_text(a).contains("hello-vim")));
    assert_eq!(app.input_mode(), Mode::Raw);
    assert_eq!(app.input_text(), "", "nothing waits in a box");
    assert!(no_gutter_ink(&app), "a full-screen program has no mark");
    capture(&app.view(), "inline-vim.png");
}

/// With the box off a paste at the prompt is not held in a box: it goes to the
/// shell under the paste rules for a program reading keys.
#[cfg(feature = "links-clipboard")]
#[test]
fn with_the_box_off_a_paste_at_the_prompt_goes_to_the_shell() {
    let mut app = zsh();
    app.set_input_box(false);
    assert_eq!(
        app.paste("echo pasted-$((5+5))").ok(),
        Some(lumen_term::app::PasteOutcome::Sent)
    );
    assert_eq!(app.input_text(), "", "nothing waits in a box");
    assert!(
        until(&mut app, |a| grid_text(a).contains("echo pasted-$((5+5))")),
        "the shell has the pasted text on its line"
    );
    enter(&mut app);
    assert!(until(&mut app, |a| grid_text(a).contains("pasted-10")));
}
