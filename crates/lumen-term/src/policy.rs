//! The user's policy file: the notification settings, and nothing else yet.
//!
//! Policy is read only from a user-owned file whose mode does not allow writes
//! by group or others. It is never read from the current directory, the
//! environment, the command line, a frame or a remote host: no program, and no
//! launcher script, can turn a setting on. A file that fails the checks is
//! ignored with a notice and the defaults apply, and so is a file with a line
//! this code does not understand, since a half-read policy is worse than none.
//!
//! The file is `$HOME/.config/lumen/policy.toml`. Its syntax is the small part
//! of TOML the settings need: `key = true` or `key = false` on a line, `#`
//! comments and blank lines. The keys are the notification settings
//! `allow_unsigned`, `os_notifications` and `os_text`, each off by default.
//!
//! The checks are stricter than the file alone: the file must not be a symbolic
//! link, the directory that holds it must be owned by the user and not be
//! writable by group or others either (or anyone could swap the file), and it
//! must be small. On a platform without Unix file modes the file is not read
//! at all, and the defaults apply.

use crate::notify::Settings;
use std::path::{Path, PathBuf};

/// The most bytes a policy file may hold.
pub const MAX_BYTES: u64 = 4096;

/// The policy file's place under the user's home directory.
pub const RELATIVE_PATH: &str = ".config/lumen/policy.toml";

/// Why a policy file was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignored {
    /// It is a symbolic link, or not a regular file.
    NotRegular,
    /// It is not owned by the user running the terminal.
    NotOwned,
    /// Its mode allows writes by group or others.
    Writable,
    /// The directory that holds it is not owned by the user.
    DirectoryNotOwned,
    /// The directory that holds it allows writes by group or others.
    DirectoryWritable,
    /// It is larger than [`MAX_BYTES`].
    TooLarge,
    /// It could not be read.
    Unreadable,
    /// It is not valid text, or a line is not `key = true|false` with a known key.
    Invalid {
        /// The line, counting from 1.
        line: usize,
        /// What was wrong with it.
        why: &'static str,
    },
    /// This platform has no file modes to check, so the file is not read.
    Unsupported,
}

/// What loading the policy found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    /// There is no policy file: every setting is at its default.
    Defaults,
    /// The file was read and its settings apply.
    Loaded {
        /// The file.
        path: PathBuf,
        /// The settings it gave.
        settings: Settings,
    },
    /// The file exists but was not used, and the defaults apply.
    Ignored {
        /// The file.
        path: PathBuf,
        /// Why not.
        why: Ignored,
    },
}

impl Policy {
    /// The settings that apply.
    #[must_use]
    pub fn settings(&self) -> Settings {
        match self {
            Self::Loaded { settings, .. } => *settings,
            Self::Defaults | Self::Ignored { .. } => Settings::default(),
        }
    }

    /// The notice to show when the file was ignored, or `None`.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        let Self::Ignored { path, why } = self else {
            return None;
        };
        let reason = match why {
            Ignored::NotRegular => "it is a symbolic link or not a regular file".to_owned(),
            Ignored::NotOwned => "it is not owned by you".to_owned(),
            Ignored::Writable => "its mode allows writes by group or others".to_owned(),
            Ignored::DirectoryNotOwned => "its directory is not owned by you".to_owned(),
            Ignored::DirectoryWritable => {
                "its directory allows writes by group or others".to_owned()
            }
            Ignored::TooLarge => format!("it is larger than {MAX_BYTES} bytes"),
            Ignored::Unreadable => "it could not be read".to_owned(),
            Ignored::Invalid { line, why } => format!("line {line}: {why}"),
            Ignored::Unsupported => "this platform cannot check file modes yet".to_owned(),
        };
        Some(format!(
            "policy file {} ignored ({reason}); the defaults apply",
            path.display()
        ))
    }
}

/// Parse the text of a policy file.
///
/// # Errors
///
/// Returns the line and the reason for the first line that is not a blank line,
/// a comment, or `key = true|false` for a known key written once.
pub fn parse(text: &str) -> Result<Settings, (usize, &'static str)> {
    let mut settings = Settings::default();
    let mut seen = [false; 3];
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let Some((key, value)) = content.split_once('=') else {
            return Err((line, "expected key = true or key = false"));
        };
        let value = match value.trim() {
            "true" => true,
            "false" => false,
            _ => return Err((line, "the value must be true or false")),
        };
        let (slot, index) = match key.trim() {
            "allow_unsigned" => (&mut settings.allow_unsigned, 0),
            "os_notifications" => (&mut settings.os_notifications, 1),
            "os_text" => (&mut settings.os_text, 2),
            _ => return Err((line, "unknown setting")),
        };
        if std::mem::replace(&mut seen[index], true) {
            return Err((line, "the setting is given twice"));
        }
        *slot = value;
    }
    Ok(settings)
}

/// Load the policy from `$HOME/.config/lumen/policy.toml`, or the defaults if
/// there is no home directory.
#[must_use]
pub fn load() -> Policy {
    let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) else {
        return Policy::Defaults;
    };
    load_from(&Path::new(&home).join(RELATIVE_PATH))
}

/// Load the policy from `path`, applying every check.
#[must_use]
pub fn load_from(path: &Path) -> Policy {
    #[cfg(unix)]
    {
        load_as(path, rustix::process::getuid().as_raw())
    }
    #[cfg(not(unix))]
    {
        if path.exists() {
            Policy::Ignored {
                path: path.to_path_buf(),
                why: Ignored::Unsupported,
            }
        } else {
            Policy::Defaults
        }
    }
}

/// Load the policy from `path` for the user whose id is `uid`: the file and its
/// directory must be owned by that user. [`load_from`] passes the id of the
/// user running the terminal; a test passes another to see a foreign file refused.
#[cfg(unix)]
#[must_use]
pub fn load_as(path: &Path, uid: u32) -> Policy {
    unix::load(path, uid)
}

#[cfg(unix)]
mod unix {
    use super::{Ignored, MAX_BYTES, Policy, parse};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    const GROUP_OR_OTHER_WRITE: u32 = 0o022;

    fn ignored(path: &Path, why: Ignored) -> Policy {
        Policy::Ignored {
            path: path.to_path_buf(),
            why,
        }
    }

    /// Load `path` for the user `uid`.
    pub(super) fn load(path: &Path, uid: u32) -> Policy {
        let Ok(before) = std::fs::symlink_metadata(path) else {
            return Policy::Defaults;
        };
        if !before.file_type().is_file() {
            return ignored(path, Ignored::NotRegular);
        }
        let Ok(file) = std::fs::File::open(path) else {
            return ignored(path, Ignored::Unreadable);
        };
        // Check the file that was opened, and that it is the one that was looked at.
        let Ok(meta) = file.metadata() else {
            return ignored(path, Ignored::Unreadable);
        };
        if (meta.dev(), meta.ino()) != (before.dev(), before.ino()) || !meta.is_file() {
            return ignored(path, Ignored::NotRegular);
        }
        if meta.uid() != uid {
            return ignored(path, Ignored::NotOwned);
        }
        if meta.mode() & GROUP_OR_OTHER_WRITE != 0 {
            return ignored(path, Ignored::Writable);
        }
        if let Some(dir) = path.parent() {
            let Ok(d) = std::fs::metadata(dir) else {
                return ignored(path, Ignored::Unreadable);
            };
            if d.uid() != uid {
                return ignored(path, Ignored::DirectoryNotOwned);
            }
            if d.mode() & GROUP_OR_OTHER_WRITE != 0 {
                return ignored(path, Ignored::DirectoryWritable);
            }
        }
        if meta.len() > MAX_BYTES {
            return ignored(path, Ignored::TooLarge);
        }
        let mut text = String::new();
        if file.take(MAX_BYTES + 1).read_to_string(&mut text).is_err() {
            return ignored(
                path,
                Ignored::Invalid {
                    line: 1,
                    why: "not valid text",
                },
            );
        }
        match parse(&text) {
            Ok(settings) => Policy::Loaded {
                path: path.to_path_buf(),
                settings,
            },
            Err((line, why)) => ignored(path, Ignored::Invalid { line, why }),
        }
    }
}
