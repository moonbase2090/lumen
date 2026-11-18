//! Shell integration, with real shells: each shell that is installed is started
//! the way the terminal starts it, on a real PTY, with a home directory of
//! its own whose start-up files set a prompt, an alias and a variable. The
//! prompt marks arrive in order with the exit status and the working
//! directory, the user's own files work as they always did, and they are not
//! changed.
//!
//! A shell that is not installed is skipped, unless `BRIGHT_REQUIRE_SHELLS`
//! names it (a comma-separated list), which makes its absence a failure.

#![cfg(unix)]

use lumen_term::shellint::{ShellOptions, plan};
use prismattyc_emulator::{PtySession, pty_size_with_cell_pixels};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// The first of `candidates` that exists, or `name` found on `PATH`.
fn find(name: &str, candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|d| d.join(name))
                    .find(|p| p.is_file())
            })
        })
}

fn required(name: &str) -> bool {
    std::env::var("BRIGHT_REQUIRE_SHELLS")
        .is_ok_and(|list| list.split(',').any(|s| s.trim() == name))
}

/// A home directory with the user's start-up files for each shell.
struct Home {
    dir: PathBuf,
    sub: PathBuf,
}

const PROMPT: &str = "USERPROMPT> ";

impl Home {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("bright-shellint-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("a b");
        std::fs::create_dir_all(&sub).unwrap_or_else(|e| unreachable!("home: {e}"));
        let put = |relative: &str, text: &str| {
            let path = dir.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| unreachable!("dir: {e}"));
            }
            std::fs::write(path, text).unwrap_or_else(|e| unreachable!("file: {e}"));
        };
        put(".zshenv", "export USER_ENV_FILE=zshenv-ran\n");
        put(
            ".zshrc",
            &format!("HISTFILE=/dev/null\nPS1='{PROMPT}'\nalias hello='echo user-alias-works'\n"),
        );
        put(
            ".bashrc",
            &format!(
                "export USER_ENV_FILE=bashrc-ran\nHISTFILE=/dev/null\nPS1='{PROMPT}'\nalias hello='echo user-alias-works'\n"
            ),
        );
        put(
            ".config/fish/config.fish",
            &format!(
                "set -gx USER_ENV_FILE config-ran\nfunction fish_prompt\n    echo -n '{PROMPT}'\nend\nalias hello 'echo user-alias-works'\n"
            ),
        );
        put(
            ".config/powershell/Microsoft.PowerShell_profile.ps1",
            &format!(
                "$env:USER_ENV_FILE = 'profile-ran'\nfunction prompt {{ '{PROMPT}' }}\nfunction hello {{ 'user-alias-works' }}\n"
            ),
        );
        Self { dir, sub }
    }

    /// Every file under the home, with its bytes, for the before and after.
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    out.insert(path.clone(), Vec::new());
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.dir, &mut out);
        out
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What a shell wrote while it ran `lines`, one after another.
fn run(program: &Path, home: &Home, integration: bool, lines: &[&str]) -> Vec<u8> {
    let program = program.display().to_string();
    let var = |name: &str| std::env::var(name).ok();
    let launch = plan(&program, &[], ShellOptions { integration }, &var);
    let mut environment: BTreeMap<String, String> = launch.environment.clone();
    environment.insert("HOME".to_owned(), home.dir.display().to_string());
    environment.insert(
        "XDG_CONFIG_HOME".to_owned(),
        home.dir.join(".config").display().to_string(),
    );
    environment.insert("TERM".to_owned(), "xterm-256color".to_owned());
    let size = pty_size_with_cell_pixels(100, 30, 8, 16);
    let mut session = PtySession::spawn_config(
        &launch.program,
        &launch.arguments,
        Some(&home.dir),
        &environment,
        size,
    )
    .unwrap_or_else(|e| unreachable!("the shell starts: {e}"));
    let mut reader = session
        .take_reader()
        .unwrap_or_else(|e| unreachable!("reader: {e}"));
    let mut writer = session
        .take_input_writer()
        .unwrap_or_else(|e| unreachable!("writer: {e}"));
    let (send, receive) = channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || send.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    let take = |until: &[u8], wait: Duration, seen: &mut Vec<u8>, writer: &mut dyn Write| {
        let end = Instant::now() + wait;
        while Instant::now() < end && !contains(seen, until) {
            if let Ok(chunk) = receive.recv_timeout(Duration::from_millis(50)) {
                answer_cursor_queries(&chunk, writer);
                seen.extend(chunk);
            }
        }
    };
    // The first prompt.
    take(
        PROMPT.as_bytes(),
        Duration::from_secs(30),
        &mut seen,
        &mut writer,
    );
    for (n, line) in lines.iter().enumerate() {
        let _ = writer.write_all(format!("{line}\r").as_bytes());
        let _ = writer.flush();
        // Wait for the prompt that follows this command.
        let wanted = seen_count(&seen, PROMPT.as_bytes()) + 1;
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end && seen_count(&seen, PROMPT.as_bytes()) < wanted {
            if let Ok(chunk) = receive.recv_timeout(Duration::from_millis(50)) {
                answer_cursor_queries(&chunk, &mut writer);
                seen.extend(chunk);
            }
        }
        let _ = n;
    }
    let _ = writer.write_all(b"exit\r");
    let _ = writer.flush();
    take(b"\0never", Duration::from_secs(2), &mut seen, &mut writer);
    session.kill_and_reap();
    drop(launch);
    seen
}

/// A terminal answers the cursor-position query (`ESC [ 6 n`) some shells send;
/// PowerShell's line editor draws nothing sensible without the answer.
fn answer_cursor_queries(chunk: &[u8], writer: &mut dyn Write) {
    for _ in 0..seen_count(chunk, b"\x1b[6n") {
        let _ = writer.write_all(b"\x1b[1;1R");
    }
    let _ = writer.flush();
}

/// The command that prints the variable the user's start-up file sets.
fn var_command(name: &str) -> &'static str {
    if name.starts_with("pwsh") {
        "$env:USER_ENV_FILE"
    } else {
        "echo $USER_ENV_FILE"
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    seen_count(haystack, needle) > 0
}

fn seen_count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// The marks in `bytes`, in order: `A`, `B`, `C`, `D;n`, or `7;url`.
fn marks(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b']' {
            let rest = &bytes[i + 2..];
            let end = rest
                .iter()
                .position(|b| *b == 0x07 || *b == 0x1b)
                .unwrap_or(rest.len());
            let body = String::from_utf8_lossy(&rest[..end]).into_owned();
            if let Some(m) = body.strip_prefix("133;") {
                out.push(m.to_owned());
            } else if body.starts_with("7;") {
                out.push(body);
            }
            i += 2 + end;
        } else {
            i += 1;
        }
    }
    out
}

fn position(list: &[String], what: &str, from: usize) -> Option<usize> {
    list.iter()
        .skip(from)
        .position(|m| m == what || m.starts_with(&format!("{what};")))
        .map(|p| p + from)
}

/// Run the common script in `program` and check everything the ticket asks for.
fn check(name: &str, program: &Path) {
    let home = Home::new(name);
    let before = home.snapshot();
    let sub = home.sub.display().to_string();
    let cd = format!("cd '{sub}'");
    let out = run(
        program,
        &home,
        true,
        &["hello", "sh -c 'exit 3'", &cd, var_command(name)],
    );
    let text = String::from_utf8_lossy(&out).into_owned();
    // The user's files work: their prompt, alias and variable.
    assert!(
        text.contains(PROMPT),
        "{name}: the user's prompt shows\n{text}"
    );
    assert!(
        text.contains("user-alias-works"),
        "{name}: the user's alias works\n{text}"
    );
    assert!(
        text.contains("-ran"),
        "{name}: the user's variable was set by their file\n{text}"
    );
    // The marks, in order, around the first command (`hello`) and the second.
    let list = marks(&out);
    let a = position(&list, "A", 0).unwrap_or(usize::MAX);
    let b = position(&list, "B", a.saturating_add(1)).unwrap_or(usize::MAX);
    let c = position(&list, "C", b.saturating_add(1)).unwrap_or(usize::MAX);
    let d = position(&list, "D", c.saturating_add(1)).unwrap_or(usize::MAX);
    assert!(
        d != usize::MAX && a < b && b < c && c < d,
        "{name}: A, B, C, D in order: {list:?}"
    );
    assert_eq!(
        list[d], "D;0",
        "{name}: a command that succeeded ends with 0: {list:?}"
    );
    assert!(
        list.iter().any(|m| m == "D;3"),
        "{name}: the exit status of a failing command is reported: {list:?}"
    );
    assert!(
        list.iter()
            .any(|m| m.starts_with("7;file://") && m.ends_with("/a%20b")),
        "{name}: the working directory arrives, percent-encoded: {list:?}"
    );
    // The user's own files are the same bytes they were.
    let after = home.snapshot();
    let changed: Vec<&PathBuf> = before
        .iter()
        .filter(|(path, bytes)| !path.is_dir() && after.get(*path) != Some(bytes))
        .map(|(path, _)| path)
        .collect();
    assert!(
        changed.is_empty(),
        "{name}: the user's files changed: {changed:?}"
    );
    // The shell may keep state of its own (history, a completion cache). The
    // hooks add nothing to what it keeps with the integration off.
    let off = Home::new(&format!("{name}-baseline"));
    let _ = run(
        program,
        &off,
        false,
        &["hello", "sh -c 'exit 3'", &cd, var_command(name)],
    );
    let names = |s: &BTreeMap<PathBuf, Vec<u8>>, root: &Path| -> Vec<PathBuf> {
        s.keys()
            .filter_map(|k| k.strip_prefix(root).ok().map(Path::to_path_buf))
            .collect()
    };
    assert_eq!(
        names(&after, &home.dir),
        names(&off.snapshot(), &off.dir),
        "{name}: the same files exist with and without the hooks"
    );
}

/// The same shell with the integration off: no marks, and the user's files work.
fn check_off(name: &str, program: &Path) {
    let home = Home::new(&format!("{name}-off"));
    let out = run(program, &home, false, &["hello"]);
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(
        text.contains("user-alias-works"),
        "{name}: works unchanged\n{text}"
    );
    assert!(
        marks(&out).is_empty(),
        "{name}: no marks with the setting off: {:?}",
        marks(&out)
    );
}

fn each(name: &str, candidates: &[&str], body: impl Fn(&str, &Path)) {
    let found = find(name, candidates);
    match found {
        Some(path) => body(name, &path),
        None => assert!(!required(name), "{name} is required but not installed"),
    }
}

#[test]
fn zsh_gets_marks_and_the_users_files_still_work() {
    each("zsh", &["/bin/zsh", "/usr/bin/zsh"], |n, p| {
        check(n, p);
        check_off(n, p);
    });
}

#[test]
fn every_installed_bash_gets_marks_and_the_users_files_still_work() {
    let mut any = false;
    for candidate in [
        "/bin/bash",
        "/usr/bin/bash",
        "/opt/homebrew/bin/bash",
        "/usr/local/bin/bash",
    ] {
        let path = Path::new(candidate);
        if path.is_file() {
            any = true;
            let name = format!("bash-{}", candidate.replace('/', "_"));
            check(&name, path);
            check_off(&name, path);
        }
    }
    assert!(
        any || !required("bash"),
        "bash is required but not installed"
    );
}

#[test]
fn fish_gets_marks_and_the_users_files_still_work() {
    each(
        "fish",
        &[
            "/usr/bin/fish",
            "/opt/homebrew/bin/fish",
            "/usr/local/bin/fish",
        ],
        |n, p| {
            check(n, p);
            check_off(n, p);
        },
    );
}

#[test]
fn powershell_gets_marks_and_the_users_files_still_work() {
    each(
        "pwsh",
        &[
            "/usr/bin/pwsh",
            "/opt/homebrew/bin/pwsh",
            "/usr/local/bin/pwsh",
            "/opt/microsoft/powershell/7/pwsh",
        ],
        |n, p| {
            check(n, p);
            check_off(n, p);
        },
    );
}

#[test]
fn another_shell_runs_unchanged_with_no_marks() {
    let Some(sh) = find("sh", &["/bin/sh"]) else {
        return;
    };
    let launch = plan(
        &sh.display().to_string(),
        &[],
        ShellOptions::default(),
        &|n| std::env::var(n).ok(),
    );
    assert!(
        launch.environment.is_empty() && launch.arguments.is_empty() && launch.scratch.is_none()
    );
}
