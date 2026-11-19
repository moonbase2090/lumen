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
    // name it was run under does not. No Enter follows.
    line(
        &mut app,
        "exec -a replaced zsh -f ~/replace.zsh $_lumen_fd $((_lumen_id + 1))",
    );
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
    for n in 1..=10 {
        line(&mut app, &format!("echo fast-{n}-done"));
    }
    assert!(
        until(&mut app, |a| grid_text(a).contains("fast-10-done\n")
            && a.input_mode() == Mode::Editing),
        "every line ran: {}",
        grid_text(&app)
    );
    let text = grid_text(&app);
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
