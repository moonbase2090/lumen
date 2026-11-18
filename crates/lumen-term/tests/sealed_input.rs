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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Fakes `yama`, `perf_event_paranoid` and `legacy_tiocsti`; reads the rest.
struct Sysctls {
    real: FsProbe,
}

impl Probe for Sysctls {
    fn is_linux(&self) -> bool {
        self.real.is_linux()
    }
    fn yama_scope(&self) -> Option<u32> {
        Some(1)
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
fn home(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    assert!(std::fs::write(dir.join(".zshrc"), "PS1='% '\n").is_ok());
    assert!(std::fs::write(dir.join(".zshenv"), "setopt no_global_rcs\n").is_ok());
    dir
}

fn start(name: &str) -> Option<App> {
    let shell = shell_or_skip()?;
    let home = home(name);
    let mut environment = BTreeMap::new();
    environment.insert("HOME".to_owned(), home.display().to_string());
    environment.insert("ZDOTDIR".to_owned(), home.display().to_string());
    environment.insert("PS1".to_owned(), "% ".to_owned());
    let config = Config {
        holder: holder(),
        listed: LISTED.iter().map(|s| (*s).to_owned()).collect(),
        probe: Arc::new(Sysctls {
            real: FsProbe::default(),
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
    assert_eq!(app.session().seal_status(), Status::Sealed);
    Some(app)
}

fn until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        app.pump();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
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
    line(&mut app, "echo reading; read -k k; echo got-$k-done");
    // While `read -k` waits, the shell is in front with ICANON off, which is
    // all a layer without sealing has to go on. The sealed command start,
    // which came before "reading", says the shell is not at a prompt, so keys
    // go to it.
    assert!(
        until(&mut app, |a| grid_text(a).contains("reading\n")
            && kernel_says_prompt(a)
            && a.input_mode() == Mode::Raw),
        "read -k waits with the kernel facts of a prompt and the box raw"
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

/// sh reads its line cooked, in front: the box is raw for it.
fn sh_reads_its_line(app: &App) -> bool {
    let tty = app.session().tty_state();
    tty.shell_in_front == Some(true) && tty.icanon == Some(true)
}

#[test]
fn a_shell_that_executes_another_program_loses_its_keys_at_its_command_start() {
    let Some(mut app) = start("sealed-input-exec-start") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    line(&mut app, "exec /bin/sh");
    // The sealed command start is checked when it arrives; by then the shell
    // has executed sh. No further Enter is needed.
    assert!(
        until(&mut app, |a| matches!(
            a.session().seal_status(),
            Status::GridOnly(_)
        )),
        "the replaced shell lost its keys: {:?}",
        app.session().seal_status()
    );
    assert_eq!(app.session().holder_pid(), None, "the holder has ended");
}

#[test]
fn a_shell_that_executes_another_program_loses_its_keys_at_the_next_enter() {
    let Some(mut app) = start("sealed-input-exec-enter") else {
        return;
    };
    assert!(until(&mut app, |a| a.input_mode() == Mode::Editing));
    // The command start comes while zsh is still zsh (sleep runs); the exec
    // comes after it, with no frame of its own.
    line(&mut app, "sleep 1; exec /bin/sh");
    assert!(
        until(&mut app, |a| sh_reads_its_line(a)
            && a.input_mode() == Mode::Raw),
        "sh reads its line"
    );
    assert_eq!(
        app.session().seal_status(),
        Status::Sealed,
        "nothing has checked the shell since its command start"
    );
    key(&mut app, Key::Enter);
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
