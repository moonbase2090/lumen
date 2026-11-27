//! The Settings command opens the settings file with its default app: through
//! `open` on macOS and `xdg-open` on Linux and the other Unix desktops. If the
//! file is absent, Bright creates a valid starter first; an existing file is
//! never replaced. Without a home directory nothing is created and the command
//! says why instead of opening anything.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

const STARTER: &str = "# Bright settings. Every key is optional.\n\
theme = \"system\"\n\
mono_font = \"bundled\"\n\
mono_size = 14\n\
prose_font = \"bundled\"\n\
prose_size = 14\n\
copy_on_select = true\n\
smooth_scroll = true\n\
input_box = true\n\
shell_integration = true\n";

/// The program that opens a file with its default app on Linux and the other
/// Unix desktops.
#[cfg(all(unix, not(target_os = "macos")))]
const OPENER: &str = "xdg-open";

fn path(home: Option<OsString>) -> Result<PathBuf, String> {
    crate::settings::home_dir(home, None, false)
        .map(|home| home.join(crate::settings::RELATIVE_PATH))
        .ok_or_else(|| "there is no home directory (HOME is not set)".to_owned())
}

fn ensure(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("settings path has no parent directory".to_owned());
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    file.write_all(STARTER.as_bytes())
        .map_err(|e| e.to_string())
}

/// Open the settings file, creating the starter first if it is absent.
///
/// On Linux the opener runs on its own: the returned receiver gets a message
/// if it fails, and closes without one when it succeeds, so the window never
/// waits on it.
///
/// # Errors
///
/// Returns why the file could not be opened: there is no home directory, the
/// file could not be created, or the opener could not be started.
pub(crate) fn open() -> Result<Option<Receiver<String>>, String> {
    if cfg!(windows) {
        return Err("opening it is not available on Windows yet".to_owned());
    }
    let path = path(std::env::var_os("HOME"))?;
    ensure(&path)?;
    #[cfg(target_os = "macos")]
    return open_on_macos(&path).map(|()| None);
    #[cfg(all(unix, not(target_os = "macos")))]
    return start(OPENER, &path).map(Some);
    #[cfg(not(unix))]
    Ok(None)
}

/// Open the file on macOS: with the app registered for it, else in the default
/// text editor, else by showing it in Finder. A Mac with no app registered for
/// `.toml` files makes the first step fail, and the later steps still leave the
/// person with the file.
#[cfg(target_os = "macos")]
fn open_on_macos(path: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    let path = path.as_os_str();
    try_in_turn(
        "/usr/bin/open",
        &[
            &[path],
            &[OsStr::new("-t"), path],
            &[OsStr::new("-R"), path],
        ],
    )
}

/// Run `program` with each argument list in turn until one succeeds. If none
/// does, the error says why each one failed, so the window can show it.
#[cfg(any(test, target_os = "macos"))]
fn try_in_turn(program: &str, attempts: &[&[&std::ffi::OsStr]]) -> Result<(), String> {
    let mut failures = Vec::new();
    for args in attempts {
        let outcome = std::process::Command::new(program)
            .args(*args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .output();
        match outcome {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => {
                let said = String::from_utf8_lossy(&out.stderr);
                let said = said.trim();
                failures.push(if said.is_empty() {
                    format!("exited with {}", out.status)
                } else {
                    said.to_owned()
                });
            }
            Err(e) => failures.push(format!("could not be started ({e})")),
        }
    }
    Err(format!(
        "{program} failed every way: {}",
        failures.join("; ")
    ))
}

/// Start `program` on `path` and watch it on a thread of its own. The receiver
/// gets a message if it ends in failure and closes without one on success.
#[cfg(any(all(test, unix), all(unix, not(target_os = "macos"))))]
fn start(program: &str, path: &Path) -> Result<Receiver<String>, String> {
    let mut child = std::process::Command::new(program)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program} could not be started ({e})"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    let program = program.to_owned();
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let _ = tx.send(format!("{program} exited with {status}"));
        }
        Err(e) => {
            let _ = tx.send(format!("{program} could not be waited for ({e})"));
        }
    });
    Ok(rx)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::start;
    use super::{STARTER, ensure, path, try_in_turn};
    use crate::settings::{LookSettings, Settings, load_from, parse};
    use std::ffi::OsStr;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn the_starter_is_valid_settings() {
        assert_eq!(
            parse(STARTER, Path::new("/settings"), None),
            Ok(LookSettings::default())
        );
    }

    #[test]
    fn the_command_creates_a_starter_and_keeps_an_existing_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!(
            "lumen-settings-command-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/settings.toml");
        ensure(&path)?;
        assert!(matches!(load_from(&path, None), Settings::Loaded { .. }));

        let custom = "theme = \"dark\"\n";
        std::fs::write(&path, custom)?;
        ensure(&path)?;
        assert_eq!(std::fs::read_to_string(path)?, custom);
        Ok(())
    }

    #[test]
    fn without_a_home_directory_the_command_says_so_instead_of_opening_anything() {
        for home in [None, Some(OsString::new())] {
            let why = path(home).err();
            assert_eq!(
                why.as_deref(),
                Some("there is no home directory (HOME is not set)")
            );
        }
    }

    // Real Unix programs stand in for the opener, which runs only on Unix.
    #[cfg(unix)]
    #[test]
    fn the_opener_is_given_the_file_and_a_failure_comes_back_as_a_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("lumen-settings-opener-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("settings.toml");
        std::fs::write(&file, "")?;

        // A program that succeeds leaves nothing to report: the channel
        // closes with no message once it has ended.
        let done = start("/bin/ls", &file)?;
        assert_eq!(done.recv().ok(), None);

        // One that fails is reported, with its exit status.
        let failed = start("/bin/cat", &dir.join("absent.toml"))?;
        let message = failed.recv()?;
        assert!(message.starts_with("/bin/cat exited with"), "{message}");

        // One that is not there at all is reported at once.
        let missing = start("lumen-no-such-opener", &file).err();
        assert!(
            missing
                .as_deref()
                .is_some_and(|m| m.starts_with("lumen-no-such-opener could not be started")),
            "{missing:?}"
        );
        Ok(())
    }

    // Real Unix programs stand in for the opener: `test -e` fails for a missing
    // path and succeeds for one that is there.
    #[cfg(unix)]
    #[test]
    fn a_later_way_of_opening_the_file_is_used_when_the_first_fails()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("lumen-settings-turns-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let there = dir.join("settings.toml");
        std::fs::write(&there, "")?;
        let absent = dir.join("absent.toml");
        let (there, absent) = (there.as_os_str(), absent.as_os_str());

        // The first way fails and the second works: the command succeeds.
        assert_eq!(
            try_in_turn(
                "/bin/test",
                &[&[OsStr::new("-e"), absent], &[OsStr::new("-e"), there]]
            ),
            Ok(())
        );
        // The first way works: the later ones are not needed.
        assert_eq!(
            try_in_turn("/bin/test", &[&[OsStr::new("-e"), there]]),
            Ok(())
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn when_every_way_fails_the_error_says_so_for_each() -> Result<(), Box<dyn std::error::Error>> {
        let why = try_in_turn(
            "/bin/ls",
            &[
                &[OsStr::new("/lumen-no-such-a")],
                &[OsStr::new("/lumen-no-such-b")],
            ],
        )
        .err()
        .ok_or("expected a failure")?;
        assert!(why.starts_with("/bin/ls failed every way: "), "{why}");
        assert!(
            why.contains("lumen-no-such-a") && why.contains("lumen-no-such-b"),
            "{why}"
        );

        let missing = try_in_turn("lumen-no-such-opener", &[&[OsStr::new("x")]]).err();
        assert!(
            missing
                .as_deref()
                .is_some_and(|m| m.contains("could not be started")),
            "{missing:?}"
        );
        Ok(())
    }
}
