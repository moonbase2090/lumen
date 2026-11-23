//! The input box on a sealed layer, on Linux, against a real zsh with its real
//! holder: the box takes keys only while the shell's last sealed boundary is a
//! prompt, and a shell that executes another program loses its keys at the
//! next Enter the window forwards.
//!
//! As in the sealed shell test, a probe fakes only the three sysctls the
//! conditions read; everything about the shell process is read for real.

#![cfg(target_os = "linux")]

use lumen_seal::{FsProbe, Probe, Sealer};
use lumen_term::input_line::Mode;
use lumen_term::mouse::Held;
use lumen_term::seal::{Config, LISTED, Status};
use lumen_term::{App, Key, Layer, Mods};
use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Fakes `yama`, `perf_event_paranoid` and `legacy_tiocsti`; reads the rest.
struct Sysctls {
    real: FsProbe,
    yama: u32,
}

impl Probe for Sysctls {
    fn is_linux(&self) -> bool {
        self.real.is_linux()
    }
    fn yama_scope(&self) -> Option<u32> {
        Some(self.yama)
    }
    fn legacy_tiocsti(&self) -> Option<u32> {
        Some(0)
    }
    fn perf_paranoid(&self) -> Option<i64> {
        Some(2)
    }
    fn proc_text(&self, pid: u32, name: &str) -> Option<String> {
        self.real.proc_text(pid, name)
    }
    fn initial_user_namespace(&self, pid: u32) -> Option<bool> {
        self.real.initial_user_namespace(pid)
    }
    fn shell_alive(&self, pidfd: Option<lumen_seal::PidFd>) -> Option<bool> {
        self.real.shell_alive(pidfd)
    }
}

/// A zsh to test with, or a reason to skip. CI has one, and a missing one there
/// is a failure.
fn shell_or_skip() -> Option<String> {
    let found = LISTED
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .map(|p| (*p).to_owned());
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "zsh is not installed on this CI machine"
        );
        eprintln!("skipped: no listed zsh on this machine");
    }
    found
}

/// The holder executable, from the build itself (see the sealed shell test).
fn holder() -> PathBuf {
    static HOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOLDER
        .get_or_init(|| {
            let out = std::process::Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-p",
                    "lumen-holder",
                    "--features",
                    "seal-requests",
                    "--message-format=json",
                ])
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap_or_else(|e| unreachable!("cargo runs: {e}"));
            assert!(out.status.success(), "the holder is built");
            let text = String::from_utf8_lossy(&out.stdout);
            let path = text
                .lines()
                .filter(|l| l.contains("\"target\":{") && l.contains("\"name\":\"lumen-holder\""))
                .filter_map(|l| l.split("\"executable\":\"").nth(1))
                .filter_map(|r| r.split('"').next())
                .next_back()
                .map(PathBuf::from)
                .unwrap_or_default();
            assert!(path.is_file(), "the holder is at {}", path.display());
            path
        })
        .clone()
}

/// A home with startup files that keep the machine's own out of the test.
fn home(name: &str, rc: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    assert!(std::fs::write(dir.join(".zshrc"), format!("PS1='% '\n{rc}")).is_ok());
    assert!(std::fs::write(dir.join(".zshenv"), "setopt no_global_rcs\n").is_ok());
    dir
}

fn start(name: &str) -> Option<App> {
    start_with_rc(name, "")
}

/// [`start`] with `rc` added to the shell's own startup file.
fn start_with_rc(name: &str, rc: &str) -> Option<App> {
    let app = start_with(name, rc, 1)?;
    assert_eq!(app.session().seal_status(), Status::Sealed);
    Some(app)
}

/// Start the shell with Yama's `ptrace_scope` read as `yama`.
fn start_with(name: &str, rc: &str, yama: u32) -> Option<App> {
    let shell = shell_or_skip()?;
    let home = home(name, rc);
    let mut environment = BTreeMap::new();
    environment.insert("HOME".to_owned(), home.display().to_string());
    environment.insert("ZDOTDIR".to_owned(), home.display().to_string());
    environment.insert("PS1".to_owned(), "% ".to_owned());
    let config = Config {
        holder: holder(),
        listed: LISTED.iter().map(|s| (*s).to_owned()).collect(),
        probe: Arc::new(Sysctls {
            real: FsProbe::default(),
            yama,
        }),
        sealer: Arc::new(Sealer::new()),
        environment,
    };
    let layer: Box<dyn Layer + Send> = Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    }));
    let app = App::start_with_seal(layer, &shell, &[] as &[&str], (1, 800, 600), &config)
        .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    Some(app)
}

/// Wait for `done`, which turns true on the program's output: after each check
/// the test blocks until the program writes again. The deadline only bounds a
/// hung test; there is no polling interval.
fn until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
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

fn grid_text(app: &App) -> String {
    app.session()
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
            lumen_term::Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The live grid's text, history first, one logical line per line: a row the
/// terminal wrapped on to the next is joined to it whole, blanks in its last
/// cells included, so text that only wrapped is not taken for two lines and a
/// real space at the wrap is kept. Only the end of a logical line is trimmed.
/// A real line break is still a line break.
fn logical_text(app: &App) -> String {
    let Some(lumen_term::Block::Grid(g)) = app.session().document().blocks().last() else {
        return String::new();
    };
    let history = g.history_rows();
    let mut out = String::new();
    let mut line = String::new();
    for index in 0..history + g.size().1 {
        let cells = if index < history {
            g.history_row(index)
        } else {
            g.screen().row(index - history)
        };
        line.extend(
            cells
                .unwrap_or_default()
                .iter()
                .filter(|c| !c.wide_cont)
                .map(|c| c.character),
        );
        if !g.screen().history_line_wrapped(index) {
            out.push_str(line.trim_end());
            out.push('\n');
            line.clear();
        }
    }
    out
}

fn key(app: &mut App, key: Key) {
    app.key(key, Mods::default())
        .unwrap_or_else(|e| unreachable!("key: {e}"));
}

fn line(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, Key::Char(c));
    }
    key(app, Key::Enter);
}

/// What the box typed for each sealed command, with its status.
fn typed(app: &App) -> Vec<(String, Option<i32>)> {
    app.typed_records()
        .into_iter()
        .map(|r| (r.typed, r.status))
        .collect()
}

/// The kernel facts alone say the shell is at a prompt: it is in front and
/// reading without ICANON.
fn kernel_says_prompt(app: &App) -> bool {
    let tty = app.session().tty_state();
    tty.shell_in_front == Some(true) && tty.icanon == Some(false)
}

#[test]
fn a_builtin_reading_keys_gets_them_and_the_box_waits_for_the_sealed_prompt() {
    let Some(mut app) = start("sealed-input-read-k") else {
        return;
    };
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "the box takes keys at the sealed prompt"
    );
    // stty puts the tty in the state `read -k` reads in before the marker is
    // written, so when "reading" is drawn the shell is in front with ICANON
    // off: all a layer without sealing has to go on, and it reads as a
    // prompt. The sealed command start came before the marker too, and says
    // the shell is not at a prompt, so keys go to it.
    line(
        &mut app,
        "stty -icanon -echo; echo reading; read -k k; stty sane; echo got-$k-done",
    );
    assert!(
        until(&mut app, |a| grid_text(a).contains("reading\n")),
        "the command ran: {}",
        grid_text(&app)
    );
    assert!(
        kernel_says_prompt(&app),
        "the kernel facts read as a prompt"
    );
    assert_eq!(
        app.input_mode(),
        Mode::Raw,
        "the box does not take the keys"
    );
    key(&mut app, Key::Char('q'));
    assert!(
        until(&mut app, |a| grid_text(a).contains("got-q-done")),
        "the key reached read -k, not the box: {}",
        grid_text(&app)
    );
    assert_eq!(app.input_text(), "", "nothing was kept in the box");
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "the box takes keys again at the next sealed prompt"
    );
    assert_eq!(
        typed(&app),
        [(
            "stty -icanon -echo; echo reading; read -k k; stty sane; echo got-$k-done".to_owned(),
            Some(0)
        )],
        "the line is recorded; the key read -k took is not"
    );
}

/// A program the shell executes in place keeps the shell's pid, start time and
/// seal channel, so the holder still answers it. This one asks for a sealed
/// command start with the next id, as the shim would, writes the frame to the
/// terminal, then prints a marker. Arguments: the channel's descriptor and the
/// command id.
const REPLACEMENT: &str = r#"zmodload zsh/system
emulate -L zsh
unsetopt multibyte
be() {
  local -i v=$1 i; local out= h
  for (( i = 3; i >= 0; i-- )); do
    h=${(l:2::0:)$(( [##16] (v >> (8 * i)) & 255 ))}; printf -v h "\\x$h"; out+=$h
  done
  REPLY=$out
}
be 900; num=$REPLY; be $2; body=$REPLY
printf -v msg '\x01\x03'
msg+=$num$'\x10'$body
if syswrite -o $1 -- $msg && sysread -i $1 -t 5 -s 128 reply \
    && [[ ${reply[1,2]} == $'\x01\x04' ]]; then
  sealed=${reply[7,-1]}
  be ${#sealed}; len=$REPLY
  printf -v tag '\x8aLUMEN\x01\xf3'
  stty -opost
  print -rn -- $'\e_'$tag$'\x10'$len$sealed
  stty opost
fi
print replaced-sent
read -r _
"#;

#[test]
fn a_program_the_shell_executes_in_place_loses_the_keys_at_its_command_start() {
    let Some(mut app) = start("sealed-input-exec-start") else {
        return;
    };
    let script = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sealed-input-exec-start")
        .join("replace.zsh");
    assert!(std::fs::write(&script, REPLACEMENT).is_ok());
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // The same program under another name: the pid and start time stay, the
    // name it was run under does not. The shell's own command start comes
    // while zsh waits in `read`, so its check never meets the exec half done;
    // the marker is quoted in two parts so the command line does not hold it.
    line(
        &mut app,
        "echo exec''-next; read -r go; \
         exec -a replaced zsh -f ~/replace.zsh $_lumen_fd $((_lumen_id + 1))",
    );
    assert!(until(&mut app, |a| grid_text(a).contains("exec-next\n")));
    assert_eq!(
        app.session().seal_status(),
        Status::Sealed,
        "the shell's command start was checked while it was still zsh"
    );
    // The Enter that answers `read` is checked before it is written, while zsh
    // is still zsh; the exec follows it. No Enter follows the exec.
    line(&mut app, "go");
    assert!(
        until(&mut app, |a| grid_text(a).contains("replaced-sent\n")),
        "the replacement ran: {}",
        grid_text(&app)
    );
    // Its frame came before its marker; by the time the marker is drawn the
    // command start has been checked and the layer has no keys.
    assert!(
        matches!(app.session().seal_status(), Status::GridOnly(_)),
        "the replaced shell lost its keys: {:?}",
        app.session().seal_status()
    );
    assert_eq!(app.session().holder_pid(), None, "the holder has ended");
    assert_eq!(
        app.seal_notice(),
        Some("Sealing ended: the shell executed another program"),
        "the user is told the layer lost its keys, and why"
    );
    assert_eq!(
        app.toasts()[0].text,
        "Sealing ended: the shell executed another program"
    );
}

#[test]
fn a_shell_that_executes_another_program_loses_its_keys_at_the_next_enter() {
    let Some(mut app) = start("sealed-input-exec-enter") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // The command start comes while zsh is still zsh, waiting in `read`; the
    // exec comes after it, with no frame of its own. The prompt is quoted in
    // two parts so that the command line drawn in the grid does not hold it.
    line(
        &mut app,
        "echo waiting; read -r go; exec env PS1='sh''-ready> ' /bin/sh",
    );
    assert!(until(&mut app, |a| grid_text(a).contains("waiting\n")));
    assert_eq!(app.session().seal_status(), Status::Sealed);
    // The Enter that answers `read` is checked before it is written: zsh is
    // still zsh then.
    line(&mut app, "go");
    assert!(
        until(&mut app, |a| grid_text(a).contains("\nsh-ready>")),
        "sh drew its prompt: {}",
        grid_text(&app)
    );
    assert_eq!(
        app.session().seal_status(),
        Status::Sealed,
        "nothing has checked the shell since it executed sh"
    );
    assert_eq!(app.seal_notice(), None, "no notice while the keys are held");
    key(&mut app, Key::Enter);
    assert_eq!(
        app.seal_notice(),
        Some("Sealing ended: the running program is not the listed shell"),
        "the notice comes with the Enter that found the change"
    );
    assert!(
        matches!(app.session().seal_status(), Status::GridOnly(_)),
        "the replaced shell lost its keys at the Enter: {:?}",
        app.session().seal_status()
    );
    assert_eq!(app.session().holder_pid(), None, "the holder has ended");
}

#[test]
fn lines_typed_without_waiting_for_the_prompt_are_never_split() {
    let Some(mut app) = start("sealed-input-fast") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // At machine speed: each line is typed the moment the last was sent,
    // before the shell has read it. Every key after an Enter is the shell's
    // until it says, sealed, that it is at a prompt again.
    // The marker is quoted in two parts. The kernel echoes lines typed ahead
    // while a command runs, so an unquoted marker would be on screen before
    // its line ran, and the wait below could end early.
    for n in 1..=10 {
        line(&mut app, &format!("echo fast-{n}''-done"));
    }
    // Output can start mid-row, after the kernel's echo of lines typed ahead,
    // and wrap: the text is read as logical lines, so a line that only wrapped
    // is not taken for one that is missing.
    assert!(
        until(&mut app, |a| logical_text(a).contains("fast-10-done\n")
            && a.input_mode() == Mode::Editing),
        "every line ran: {}",
        logical_text(&app)
    );
    let text = logical_text(&app);
    for n in 1..=10 {
        assert!(
            text.contains(&format!("fast-{n}-done\n")),
            "line {n}: {text}"
        );
    }
    assert!(!text.contains("not found"), "no line was split: {text}");
    assert_eq!(app.input_text(), "", "nothing was left in the box");
    assert_eq!(app.forwarded_enters(), 10);
}

/// A shell that does not take bracketed paste, so a multi-line box asks how
/// to send it. The line editor sets the parameter when it loads, so it is
/// loaded first.
const NO_BRACKETED_PASTE: &str = "zmodload zsh/zle\nunset zle_bracketed_paste\n";

/// Type `text` as a whole into the box, press Enter (the box holds it and
/// asks), then Enter again: send as separate commands.
fn send_as_commands(app: &mut App, text: &str) {
    app.text(text).unwrap_or_else(|e| unreachable!("text: {e}"));
    key(app, Key::Enter);
    assert!(
        app.input_text().contains('\n'),
        "the lines are held for the choice: box={:?} modes={:?} mode={:?}",
        app.input_text(),
        app.session().key_modes(),
        app.input_mode()
    );
    key(app, Key::Enter);
}

#[test]
fn send_as_commands_runs_each_line_in_turn_at_its_own_prompt() {
    let Some(mut app) = start_with_rc("sealed-input-send-lines", NO_BRACKETED_PASTE) else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    send_as_commands(&mut app, "echo one-ran\necho two-ran\necho three-ran");
    assert!(
        until(&mut app, |a| grid_text(a).contains("three-ran\n")
            && a.input_mode() == Mode::Editing),
        "every line ran: {}",
        grid_text(&app)
    );
    let text = grid_text(&app);
    let at = |m: &str| text.find(m).unwrap_or(usize::MAX);
    assert!(at("one-ran\n") < at("two-ran\n") && at("two-ran\n") < at("three-ran\n"));
    assert_eq!(app.input_text(), "", "nothing was put back");
    assert_eq!(app.forwarded_enters(), 3);
    assert_eq!(app.session().seal_status(), Status::Sealed);
    assert_eq!(
        typed(&app),
        [
            ("echo one-ran".to_owned(), Some(0)),
            ("echo two-ran".to_owned(), Some(0)),
            ("echo three-ran".to_owned(), Some(0)),
        ],
        "each line is recorded for its own command; rows: {:?}\n{}",
        app.typed_records()
            .iter()
            .map(|r| app.typed_row_text(r))
            .collect::<Vec<_>>(),
        grid_text(&app)
    );
}

/// Run in place of the shell: it keeps the shell's channel and asks the holder
/// for a sealed prompt, as the shim would, writes it, prints a marker, then
/// prints the first line it reads. Argument: the channel's descriptor.
const PROMPTING_REPLACEMENT: &str = r#"zmodload zsh/system
emulate -L zsh
unsetopt multibyte
printf -v msg '\x01\x03\x00\x00\x03\x84\x11'
if syswrite -o $1 -- $msg && sysread -i $1 -t 5 -s 128 reply \
    && [[ ${reply[1,2]} == $'\x01\x04' ]]; then
  sealed=${reply[7,-1]}
  n=${#sealed}
  printf -v len '\\x%02x\\x%02x\\x%02x\\x%02x' $(( n >> 24 & 255 )) $(( n >> 16 & 255 )) $(( n >> 8 & 255 )) $(( n & 255 ))
  printf -v len "$len"
  printf -v tag '\x8aLUMEN\x01\xf3'
  stty -opost
  print -rn -- $'\e_'$tag$'\x11'$len$sealed
  stty opost
fi
print replaced-ready
read -r l
print "got:$l"
read -r _
"#;

#[test]
fn a_line_after_one_that_replaces_the_shell_is_checked_and_never_sent_to_it() {
    let name = "sealed-input-replace-then-line";
    let Some(mut app) = start_with_rc(name, NO_BRACKETED_PASTE) else {
        return;
    };
    let script = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(name)
        .join("replace.zsh");
    assert!(std::fs::write(&script, PROMPTING_REPLACEMENT).is_ok());
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // The first line replaces the shell with a program that keeps its channel
    // and gets a sealed prompt drawn; the second would go to that program.
    send_as_commands(
        &mut app,
        "exec -a replaced zsh -f ~/replace.zsh $_lumen_fd\necho second-line",
    );
    assert!(
        until(&mut app, |a| grid_text(a).contains("replaced-ready\n")),
        "the replacement ran: {}",
        grid_text(&app)
    );
    assert!(
        matches!(app.session().seal_status(), Status::GridOnly(_)),
        "the replaced shell lost its keys: {:?}",
        app.session().seal_status()
    );
    assert_eq!(
        app.input_text(),
        "echo second-line",
        "the second line came back to the box unsent"
    );
    // What the replacement reads first is what the user types now, not the
    // second line.
    for c in "probe".chars() {
        key(&mut app, Key::Char(c));
    }
    key(&mut app, Key::Enter);
    assert!(
        until(&mut app, |a| grid_text(a).contains("got:")),
        "the replacement read a line: {}",
        grid_text(&app)
    );
    let text = grid_text(&app);
    assert!(text.contains("got:probe\n"), "{text}");
    assert!(!text.contains("got:echo second-line"), "{text}");
}

/// The pixel of the first toast, found by asking the window what a press at
/// each pixel would hit.
fn first_toast_pixel(app: &App) -> Option<(u32, u32)> {
    (0..600u32)
        .flat_map(|y| (0..800u32).map(move |x| (x, y)))
        .find(|&(x, y)| app.click_would_hit(x, y) == Some(0))
}

#[test]
fn a_shell_started_without_keys_says_why_until_the_user_dismisses_it() {
    let Some(mut app) = start_with("sealed-input-grid-only", "", 0) else {
        return;
    };
    assert_eq!(
        app.session().seal_status(),
        Status::GridOnly("ptrace_scope is 0, below 1".to_owned())
    );
    assert_eq!(
        app.seal_notice(),
        Some("Shell not sealed: ptrace_scope is 0, below 1")
    );
    // Without keys the box follows the kernel's signals alone, and takes keys
    // once the shell is at its prompt.
    assert!(
        until(&mut app, |a| a.input_mode() == Mode::Editing),
        "the shell runs: {}",
        grid_text(&app)
    );
    // Quoted in two parts, so the command line drawn in the grid does not hold
    // the marker.
    line(&mut app, "echo still''-here");
    assert!(until(&mut app, |a| grid_text(a).contains("still-here\n")));
    assert!(!app.tick());
    assert_eq!(
        app.seal_notice(),
        Some("Shell not sealed: ptrace_scope is 0, below 1"),
        "it stays: it has no time limit and output does not remove it"
    );
    let Some((x, y)) = first_toast_pixel(&app) else {
        unreachable!("the notice is drawn: {:?}", app.toasts())
    };
    assert!(app.click(x, y), "a press on the notice dismisses it");
    assert_eq!(app.seal_notice(), None);
    assert_eq!(app.toasts(), []);
}

#[test]
fn a_shell_that_exits_leaves_no_sealing_notice() {
    let Some(mut app) = start("sealed-input-exit") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    let holder = app
        .session()
        .holder_pid()
        .unwrap_or_else(|| unreachable!("a sealed layer has a holder"));
    let holder = lumen_holder::ffi_linux::pidfd_open(holder)
        .unwrap_or_else(|e| unreachable!("a pidfd for the holder: {e}"));
    line(&mut app, "exit");
    // The holder ends when the shell does, so the layer has no keys. It has
    // closed, too, and a closed layer has no notice. Wait for both ends.
    // The pidfd turns readable when the holder exits; the limit only bounds a
    // hung test.
    let ended = lumen_holder::ffi_linux::wait_with_timeout(&[holder.as_raw_fd()], 30_000)
        .is_ok_and(|e| e.first().is_some_and(|e| e.readable));
    assert!(ended, "the holder ended");
    assert!(until(&mut app, App::has_exited), "the shell exited");
    app.tick();
    assert_eq!(app.seal_notice(), None);
    assert_eq!(app.toasts(), []);
}

/// Type `s` and Enter, then wait until the shell is back at its prompt and the
/// box takes keys again, and `marker` (written in two parts in `s`) is drawn.
fn run(app: &mut App, s: &str, marker: &str) {
    line(app, s);
    assert!(
        until(app, |a| grid_text(a).contains(&format!("{marker}\n"))
            && a.input_mode() == Mode::Editing),
        "{s:?} ran: {}",
        grid_text(app)
    );
}

#[test]
fn a_line_sent_from_the_box_is_recorded_with_its_sealed_status() {
    let Some(mut app) = start("sealed-input-typed") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo typed''-one", "typed-one");
    run(&mut app, "echo typed''-two; false", "typed-two");
    let records = app.typed_records();
    assert_eq!(
        typed(&app),
        [
            ("echo typed''-one".to_owned(), Some(0)),
            ("echo typed''-two; false".to_owned(), Some(1)),
        ]
    );
    assert!(
        records[0].id > 0 && records[1].id == records[0].id + 1,
        "the ids are the sealed command ids: {records:?}"
    );
}

#[test]
fn a_line_handed_to_the_shell_is_not_recorded() {
    let Some(mut app) = start("sealed-input-handed") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // Tab hands the box's text to the shell's own line editor; the Enter after
    // it is the shell's, and the line is the shell's too.
    for c in "echo handed''-off".chars() {
        key(&mut app, Key::Char(c));
    }
    key(&mut app, Key::Tab);
    assert_eq!(app.input_mode(), Mode::HandedOff);
    key(&mut app, Key::Enter);
    assert!(
        until(&mut app, |a| grid_text(a).contains("handed-off\n")
            && a.input_mode() == Mode::Editing),
        "the line ran: {}",
        grid_text(&app)
    );
    assert_eq!(typed(&app), [], "nothing typed in the box whole");
    run(&mut app, "echo after''-it", "after-it");
    assert_eq!(typed(&app), [("echo after''-it".to_owned(), Some(0))]);
}

#[test]
fn up_recalls_only_lines_that_started_a_sealed_command() {
    let Some(mut app) = start("sealed-input-recall") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo recall''-me", "recall-me");
    // A line the shell will not parse: no command starts, and the shell is
    // back at its prompt. The box's own history has it.
    line(&mut app, ")");
    assert!(
        until(&mut app, |a| grid_text(a).contains("parse error")
            && a.input_mode() == Mode::Editing),
        "the shell refused the line: {}",
        grid_text(&app)
    );
    assert_eq!(typed(&app), [("echo recall''-me".to_owned(), Some(0))]);
    key(&mut app, Key::Up);
    assert_eq!(app.input_text(), "echo recall''-me", "Edit again");
    key(&mut app, Key::Up);
    assert_eq!(app.input_text(), "echo recall''-me", "nothing older");
    key(&mut app, Key::Down);
    assert_eq!(app.input_text(), "", "Down past the newest empties the box");
}

#[test]
fn a_layer_without_keys_records_nothing_and_up_recalls_the_box_history() {
    let Some(mut app) = start_with("sealed-input-recall-grid", "", 0) else {
        return;
    };
    assert!(matches!(app.session().seal_status(), Status::GridOnly(_)));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo grid''-only", "grid-only");
    line(&mut app, ")");
    assert!(
        until(&mut app, |a| grid_text(a).contains("parse error")
            && a.input_mode() == Mode::Editing),
        "the shell refused the line: {}",
        grid_text(&app)
    );
    assert_eq!(typed(&app), [], "no sealed frames, no records");
    key(&mut app, Key::Up);
    assert_eq!(app.input_text(), ")", "the box's own history");
}

#[test]
fn a_record_points_at_the_row_its_line_was_typed_on_until_the_screen_is_cleared() {
    let Some(mut app) = start("sealed-input-anchor") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo anchor''-me", "anchor-me");
    let records = app.typed_records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        app.typed_row_text(&records[0]).as_deref(),
        Some("echo anchor''-me"),
        "the row the typed line is on"
    );
    // Push it well into history.
    run(
        &mut app,
        "for i in {1..120}; do echo filler-$i; done; echo fill''-done",
        "fill-done",
    );
    assert_eq!(
        app.typed_row_text(&records[0]).as_deref(),
        Some("echo anchor''-me"),
        "the same row after it scrolled into history"
    );
    // Clearing the screen and its history drops it.
    run(
        &mut app,
        "printf '\\033[H\\033[2J\\033[3J'; echo clear''-done",
        "clear-done",
    );
    assert_eq!(app.typed_row_text(&records[0]), None);
    // The line that cleared was anchored before it ran, so it goes too.
    let records = app.typed_records();
    assert_eq!(records.len(), 3);
    assert_eq!(app.typed_row_text(&records[2]), None);
}

/// The shell's command start and the command's first output in one write: the
/// shim's own hook is replaced by one that asks for the same sealed frame and
/// writes it together with a clear of the screen and its history.
const START_THEN_CLEAR: &str = r"add-zsh-hook -d preexec _lumen_preexec
_start_then_clear() {
  (( _lumen_id++, _lumen_ran = 1 ))
  _lumen_send seal 16 $_lumen_id
  _lumen_out+=$'\e[H\e[2J\e[3J'
  _lumen_flush
}
add-zsh-hook preexec _start_then_clear
";

#[test]
fn a_command_start_is_checked_against_the_screen_as_it_was_when_the_frame_came() {
    let Some(mut app) = start_with_rc("sealed-input-start-then-clear", START_THEN_CLEAR) else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // One write, so one read: the frame, then the clear. When the frame came
    // the row the line was typed on showed it, so the line is recorded; the
    // clear that came after the frame does not unmake that.
    run(&mut app, "echo after''-clear", "after-clear");
    assert_eq!(typed(&app), [("echo after''-clear".to_owned(), Some(0))]);
    // The clear did come, and took the row with it.
    assert_eq!(app.typed_row_text(&app.typed_records()[0]), None);
}

/// How many pixels of the gutter, drawn `cells` cells wide, on the row whose
/// top is pixel `top`, differ from the gutter's own colour there. Text in the
/// gutter differs; an empty gutter row has none. A layer with its keys draws
/// the gutter four cells wide, one without them two.
fn gutter_ink(frame: &lumen_term::Frame, top: u32, cells: u32) -> usize {
    let (cw, ch) = cell();
    let gutter = cw * cells - 2;
    let plain = frame.pixel(1, top + ch - 1);
    (top..top + ch)
        .flat_map(|y| (0..gutter).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.pixel(x, y) != plain)
        .count()
}

fn cell() -> (u32, u32) {
    lumen_term::Renderer::light(1)
        .unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
        .cell()
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

#[test]
fn a_typed_line_has_a_gutter_mark_that_explains_itself_and_edits_it_again() {
    let Some(mut app) = start("sealed-input-mark") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo mark''-me", "mark-me");
    let marks = app.typed_marks();
    assert_eq!(marks.len(), 1, "{marks:?}");
    let (top, record) = marks[0];
    assert_eq!(record, 0);
    assert_eq!(
        app.typed_row_text(&app.typed_records()[0]).as_deref(),
        Some("echo mark''-me")
    );
    let plain = app.view();
    capture(&plain, "typed-mark.png");
    assert!(
        gutter_ink(&plain, top, 4) > 0,
        "the mark is drawn in the gutter"
    );
    let (cw, ch) = cell();
    assert_eq!(
        gutter_ink(&plain, top + ch, 4),
        0,
        "the next row's gutter is empty"
    );

    // Over the mark: what it means, then gone when the pointer leaves.
    let on_mark = (cw, top + ch / 2);
    assert_eq!(
        app.mouse_move(on_mark.0, on_mark.1, Held::default()).ok(),
        Some(true),
        "the window draws again"
    );
    assert_eq!(app.typed_hover(), Some(0));
    let hovered = app.view();
    capture(&hovered, "typed-mark-hover.png");
    assert_ne!(hovered, plain, "the tip is drawn");
    // Not over the text it is about: the typed line's cells are as they were.
    let left = cw * lumen_term::GUTTER_COLUMNS;
    let right = left + cw * u32::try_from("echo mark''-me".len() + 1).unwrap_or(0);
    let covered = (top..top + ch)
        .flat_map(|y| (left..right).map(move |x| (x, y)))
        .filter(|&(x, y)| hovered.pixel(x, y) != plain.pixel(x, y))
        .count();
    assert_eq!(covered, 0, "the tip leaves the typed text alone");
    assert_eq!(
        header_changes(&plain, &hovered),
        0,
        "the tip stays out of the header strip"
    );
    assert_eq!(
        app.mouse_move(cw * 20, top + ch / 2, Held::default()).ok(),
        Some(true)
    );
    assert_eq!(app.typed_hover(), None);
    assert_eq!(app.view(), plain, "the tip is gone");

    // A click on the mark puts the line back in the box, and sends nothing.
    let sent = app.forwarded_enters();
    assert!(app.click(on_mark.0, on_mark.1));
    assert_eq!(app.input_text(), "echo mark''-me");
    assert_eq!(app.forwarded_enters(), sent, "nothing was sent");
    capture(&app.view(), "typed-mark-edit-again.png");
}

#[test]
fn a_layer_without_keys_draws_no_mark() {
    let Some(mut app) = start_with("sealed-input-no-mark", "", 0) else {
        return;
    };
    assert!(matches!(app.session().seal_status(), Status::GridOnly(_)));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo no''-mark", "no-mark");
    assert_eq!(app.typed_marks(), []);
    let frame = app.view();
    capture(&frame, "no-mark-grid-only.png");
    let (_, ch) = cell();
    let rows = frame.height / ch;
    for row in 1..rows {
        assert_eq!(gutter_ink(&frame, row * ch, 2), 0, "gutter row {row}");
    }
}

#[test]
fn a_typed_line_that_wraps_is_marked_on_its_first_row_and_its_tip_does_not_cover_it() {
    let Some(mut app) = start("sealed-input-wrap") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // Longer than the grid is wide, so the shell draws it over two rows.
    let long = format!("echo {}''-end", "w".repeat(130));
    run(&mut app, &long, "-end");
    let records = app.typed_records();
    assert_eq!(records.len(), 1);
    let row = app
        .typed_row_text(&records[0])
        .unwrap_or_else(|| unreachable!("the record has a row: {}", grid_text(&app)));
    assert!(
        row.starts_with("echo www") && !row.contains("-end"),
        "the first row of the typed line: {row:?}"
    );
    let marks = app.typed_marks();
    assert_eq!(marks.len(), 1);
    // The first row is full: the tip goes above it, not over it.
    let (top, _) = marks[0];
    let plain = app.view();
    let (cw, ch) = cell();
    assert_eq!(
        app.mouse_move(cw, top + ch / 2, Held::default()).ok(),
        Some(true)
    );
    let hovered = app.view();
    capture(&hovered, "typed-mark-hover-wrapped.png");
    assert_ne!(hovered, plain, "the tip is drawn");
    let left = cw * lumen_term::GUTTER_COLUMNS;
    let covered = (top..top + ch)
        .flat_map(|y| (left..hovered.width).map(move |x| (x, y)))
        .filter(|&(x, y)| hovered.pixel(x, y) != plain.pixel(x, y))
        .count();
    assert_eq!(covered, 0, "the full first row is left alone");
    assert_eq!(
        header_changes(&plain, &hovered),
        0,
        "the tip stays out of the header strip"
    );
}

/// How many pixels of the header strip differ between two frames.
fn header_changes(a: &lumen_term::Frame, b: &lumen_term::Frame) -> usize {
    let (_, ch) = cell();
    let header = ch * lumen_term::HEADER_ROWS;
    (0..header)
        .flat_map(|y| (0..a.width).map(move |x| (x, y)))
        .filter(|&(x, y)| a.pixel(x, y) != b.pixel(x, y))
        .count()
}

/// The live grid's width in columns, and the column the shell's cursor is on.
fn grid_width_and_column(app: &App) -> (usize, usize) {
    match app.session().document().blocks().last() {
        Some(lumen_term::Block::Grid(g)) => (g.size().0, g.cursor().column),
        _ => unreachable!("the live block is a grid"),
    }
}

#[test]
fn a_joined_emoji_near_the_edge_leaves_the_mark_on_the_row_the_line_starts_on() {
    let Some(mut app) = start("sealed-input-zwj") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // A family emoji: five scalars that the terminal draws as one grapheme two
    // cells wide, while a shell's line editor may draw it some other way and
    // wrap the line where the terminal would not. Whatever it does, the mark is
    // on the row the line starts on: the one row the terminal knows.
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    let (columns, column) = grid_width_and_column(&app);
    let fill = columns - column - "echo ".len() - 2 - "''-zwj".len();
    let text = format!("echo {}{family}''-zwj", "a".repeat(fill));
    run(&mut app, &text, "-zwj");
    // The record stays only if the row shows the line as typed. A shell that
    // drew the joiner its own way leaves it out: no mark rather than a wrong one.
    let records = app.typed_records();
    assert!(records.len() <= 1, "{records:?}");
    for record in &records {
        let row = app
            .typed_row_text(record)
            .unwrap_or_else(|| unreachable!("the record has a row: {}", grid_text(&app)));
        assert!(
            row.starts_with("echo aaa"),
            "the row the typed line starts on: {row:?}\n{}",
            grid_text(&app)
        );
    }
    assert_eq!(app.typed_marks().len(), records.len());
}

#[test]
fn the_tip_counts_wide_characters_in_cells() {
    let Some(mut app) = start("sealed-input-wide") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // Six characters two cells wide: the line is 24 cells, 18 characters.
    let line = "echo \u{4E2D}\u{6587}\u{4E2D}\u{6587}\u{4E2D}\u{6587}''-wide";
    run(&mut app, line, "-wide");
    let marks = app.typed_marks();
    assert_eq!(marks.len(), 1, "{marks:?}");
    let (top, _) = marks[0];
    let plain = app.view();
    let (cw, ch) = cell();
    assert_eq!(
        app.mouse_move(cw, top + ch / 2, Held::default()).ok(),
        Some(true)
    );
    let hovered = app.view();
    capture(&hovered, "typed-mark-hover-wide.png");
    assert_ne!(hovered, plain, "the tip is drawn");
    let left = cw * lumen_term::GUTTER_COLUMNS;
    let right = left + cw * 25;
    let covered = (top..top + ch)
        .flat_map(|y| (left..right).map(move |x| (x, y)))
        .filter(|&(x, y)| hovered.pixel(x, y) != plain.pixel(x, y))
        .count();
    assert_eq!(covered, 0, "the tip leaves all 24 cells of the line alone");
}

#[test]
fn leaving_a_mark_for_the_grid_still_reports_the_motion_to_a_program_that_asked() {
    let Some(mut app) = start("sealed-input-motion") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    run(&mut app, "echo mark''-first", "mark-first");
    // A program on the primary screen that asks for every motion (1003, SGR)
    // and prints the start of the first report it reads.
    line(
        &mut app,
        "printf '\\033[?1003h\\033[?1006h'; echo wait''ing; read -k 6 r; printf '\\033[?1003l\\033[?1006l'; print -r -- got-${r[2,6]}",
    );
    assert!(until(&mut app, |a| grid_text(a).contains("waiting\n")));
    let marks = app.typed_marks();
    let (top, _) = marks[0];
    let (cw, ch) = cell();
    assert_eq!(
        app.mouse_move(cw, top + ch / 2, Held::default()).ok(),
        Some(true),
        "onto the mark: the tip"
    );
    assert_eq!(app.typed_hover(), Some(0));
    // Off the mark, onto a cell of the grid: the tip goes, and the motion is
    // reported all the same.
    let on_grid = (cw * (lumen_term::GUTTER_COLUMNS + 6), top + ch * 3 + ch / 2);
    assert_eq!(
        app.mouse_move(on_grid.0, on_grid.1, Held::default()).ok(),
        Some(true)
    );
    assert_eq!(app.typed_hover(), None);
    assert!(
        until(&mut app, |a| grid_text(a).contains("got-[<35;")),
        "the program read the motion report: {}",
        grid_text(&app)
    );
}

/// Holds the shell at its next prompt, before its line editor reads anything,
/// until `~/gate2` is written, once `~/hold` exists.
const HOLD_AT_PROMPT: &str = "zle-line-init() {\n  if [[ -e ~/hold ]]; then rm -f ~/hold; read -r _ < ~/gate2; fi\n}\nzle -N zle-line-init\n";

fn mkfifo(path: &std::path::Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .is_ok_and(|s| s.success()),
        "a fifo at {}",
        path.display()
    );
}

#[test]
fn a_box_line_queued_behind_a_line_typed_ahead_is_not_recorded_for_it() {
    let Some(mut app) = start_with_rc("sealed-input-queued", HOLD_AT_PROMPT) else {
        return;
    };
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sealed-input-queued");
    mkfifo(&dir.join("gate"));
    mkfifo(&dir.join("gate2"));
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // A command that waits on the gate; its record says it has started.
    line(&mut app, "cat ~/gate; echo gate''-open");
    assert!(until(&mut app, |a| a.typed_records().len() == 1));
    // The shell will hold at its next prompt before it reads anything.
    assert!(std::fs::write(dir.join("hold"), "").is_ok());
    // Typed ahead while the command waits: queued for the shell's next line.
    line(&mut app, "echo queued''-x");
    assert_eq!(app.input_mode(), Mode::Raw, "a command is running");
    assert!(
        std::fs::write(dir.join("gate"), "x\n").is_ok(),
        "the gate opens"
    );
    // The command is over and the shell has sent its sealed prompt, but has
    // not read the queued line yet: the box takes keys.
    assert!(
        until(&mut app, |a| grid_text(a).contains("gate-open\n")
            && a.input_mode() == Mode::Editing),
        "the shell is held at its prompt: {}",
        grid_text(&app)
    );
    // The box's line goes in behind the queued one.
    line(&mut app, "echo box''-y");
    // Let the shell go on. The write waits for the hook to open the fifo, so
    // it runs on its own thread; the wait below bounds it.
    let gate2 = dir.join("gate2");
    let opener = std::thread::spawn(move || std::fs::write(gate2, "x\n").is_ok());
    assert!(
        until(&mut app, |a| grid_text(a).contains("box-y\n")
            && a.input_mode() == Mode::Editing),
        "both lines ran: {}",
        grid_text(&app)
    );
    assert!(opener.join().unwrap_or(false), "the hook read the gate");
    let text = grid_text(&app);
    let at = |m: &str| text.find(m).unwrap_or(usize::MAX);
    assert!(
        at("queued-x\n") < at("box-y\n"),
        "the line typed ahead ran first: {text}"
    );
    // The command that started after the box sent its line was the queued
    // one: the box's text is not recorded for it, and no mark is drawn.
    assert_eq!(
        typed(&app),
        [("cat ~/gate; echo gate''-open".to_owned(), Some(0))],
        "{text}"
    );
    assert_eq!(app.typed_marks().len(), 1);
}

/// At the first prompt after `~/gate` exists, the shell's line editor runs a
/// hook that waits on `~/gate` with a builtin (the shell stays in front), then
/// on `~/gate2` with a program (the program is in front until it is written).
/// The program says `held` first: the shell hands it the terminal before it
/// starts, so that output is the test's signal that it is in front.
const HOOK_RUNS_A_PROGRAM: &str = "zle-line-init() {\n  if [[ -p ~/gate && -z $_held ]]; then\n    _held=1\n    read -r _ < ~/gate\n    sh -c 'echo held; exec cat ~/gate2 > /dev/null'\n  fi\n}\nzle -N zle-line-init\n";

#[test]
fn keys_typed_while_a_prompt_hook_runs_a_program_finish_the_line_in_the_box() {
    let Some(mut app) = start_with_rc("sealed-input-hook-program", HOOK_RUNS_A_PROGRAM) else {
        return;
    };
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sealed-input-hook-program");
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    mkfifo(&dir.join("gate"));
    mkfifo(&dir.join("gate2"));
    // The next prompt holds in the hook.
    line(&mut app, "true");
    assert!(until(&mut app, |a| typed(a)
        == [("true".to_owned(), Some(0))]
        && a.input_mode() == Mode::Editing));
    for c in "echo bo".chars() {
        key(&mut app, Key::Char(c));
    }
    assert_eq!(app.input_text(), "echo bo");
    // The hook goes on to its program, which takes the tty.
    let gate = dir.join("gate");
    let opener = std::thread::spawn(move || std::fs::write(gate, "x\n").is_ok());
    assert!(
        until(&mut app, |a| grid_text(a).contains("held")
            && a.session().tty_state().shell_in_front == Some(false)),
        "the hook's program is in front"
    );
    assert!(opener.join().unwrap_or(false), "the hook read the gate");
    // The rest of the line, typed while the program is in front.
    line(&mut app, "x''-y");
    let gate2 = dir.join("gate2");
    let opener = std::thread::spawn(move || std::fs::write(gate2, "x\n").is_ok());
    assert!(
        until(&mut app, |a| grid_text(a).contains("box-y\n")
            && a.input_mode() == Mode::Editing),
        "the whole line ran: {}",
        grid_text(&app)
    );
    assert!(opener.join().unwrap_or(false), "the program read its gate");
    assert_eq!(app.input_text(), "", "nothing was left in the box");
    assert!(
        !grid_text(&app).contains("not found"),
        "no part of the line ran alone: {}",
        grid_text(&app)
    );
}

/// The gutter holds the typed-line marks only on a layer with its keys. There
/// it is drawn four cells wide; on a layer without them it is two, and the
/// program is given the columns each width leaves.
#[test]
fn the_gutter_is_full_width_only_where_it_can_hold_marks() {
    let (cw, ch) = cell();
    // A row of the block area above any typed line: no mark is drawn on it.
    let y = ch * 3 + ch / 2;
    let divider_at = |app: &App| {
        let view = app.view();
        let gutter = view.pixel(1, y);
        (1..cw * 6).find(|&x| view.pixel(x, y) != gutter)
    };
    if let Some(mut app) = start("sealed-input-gutter-full") {
        assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
        assert_eq!(divider_at(&app), Some(4 * cw - 1), "sealed: full gutter");
        // 800 pixels less the full gutter, its 6 pixel padding and the 12
        // pixel right inset leave 97 cells, 93 of them right of the gutter.
        assert_eq!(app.session().document().columns(), 93);
    }
    if let Some(mut app) = start_with("sealed-input-gutter-empty", "", 0) {
        assert!(matches!(app.session().seal_status(), Status::GridOnly(_)));
        assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
        assert_eq!(divider_at(&app), Some(2 * cw - 1), "no keys: narrow gutter");
        // The narrow gutter gives the program two more columns.
        assert_eq!(app.session().document().columns(), 95);
    }
}

/// A layer that loses its keys mid-session is drawn with the narrow gutter
/// from then on, and the program is told the two columns that frees.
#[test]
fn a_layer_that_loses_its_keys_narrows_the_gutter_and_tells_the_program() {
    let Some(mut app) = start("sealed-input-gutter-narrows") else {
        return;
    };
    let (cw, ch) = cell();
    let y = ch * 3 + ch / 2;
    let divider_at = |app: &App| {
        let view = app.view();
        let gutter = view.pixel(1, y);
        (1..cw * 6).find(|&x| view.pixel(x, y) != gutter)
    };
    // The width the program reads from its terminal, printed after a marker
    // the echoed command line does not hold.
    let told = |app: &App| {
        let text = grid_text(app);
        text.lines()
            .filter_map(|l| l.split("cols=").nth(1))
            .filter_map(|rest| rest.split_whitespace().nth(1)?.parse::<usize>().ok())
            .collect::<Vec<_>>()
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    line(&mut app, "echo co''ls=$(stty size)");
    assert!(
        until(&mut app, |a| !told(a).is_empty()),
        "{}",
        grid_text(&app)
    );
    assert_eq!(told(&app), [93], "sealed: the program has 93 columns");
    assert_eq!(divider_at(&app), Some(4 * cw - 1), "sealed: full gutter");
    // The shell replaces itself; the next Enter finds it and the keys go.
    line(&mut app, "exec env PS1='sh''-ready> ' /bin/sh");
    assert!(
        until(&mut app, |a| grid_text(a).contains("\nsh-ready>")),
        "{}",
        grid_text(&app)
    );
    key(&mut app, Key::Enter);
    assert!(
        matches!(app.session().seal_status(), Status::GridOnly(_)),
        "{:?}",
        app.session().seal_status()
    );
    assert_eq!(divider_at(&app), Some(2 * cw - 1), "no keys: narrow gutter");
    assert_eq!(app.session().document().columns(), 95);
    line(&mut app, "echo co''ls=$(stty size)");
    assert!(
        until(&mut app, |a| told(a).len() == 2),
        "{}",
        grid_text(&app)
    );
    assert_eq!(told(&app), [93, 95], "the program is told the wider grid");
}
