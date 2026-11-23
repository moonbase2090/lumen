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

#[cfg(target_os = "macos")]
fn open_on_macos(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("/usr/bin/open exited with {status}"))
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
    use super::{STARTER, ensure, path};
    use crate::settings::{LookSettings, Settings, load_from, parse};
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
}
