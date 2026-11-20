//! The macOS Settings command opens the settings file with its default app.
//! If the file is absent, Bright creates a valid starter without replacing an
//! existing file.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const STARTER: &str = "# Bright settings. Every key is optional.\n\
theme = \"system\"\n\
mono_font = \"bundled\"\n\
mono_size = 14\n\
prose_font = \"bundled\"\n\
prose_size = 14\n\
copy_on_select = true\n\
smooth_scroll = true\n\
shell_integration = true\n";

fn path() -> Result<PathBuf, String> {
    crate::settings::home_dir(std::env::var_os("HOME"), None, false)
        .map(|home| home.join(crate::settings::RELATIVE_PATH))
        .ok_or_else(|| "no home directory".to_owned())
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

pub(crate) fn open() -> Result<(), String> {
    let path = path()?;
    ensure(&path)?;
    std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{STARTER, ensure};
    use crate::settings::{LookSettings, Settings, load_from, parse};
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
}
