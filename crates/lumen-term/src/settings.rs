//! The user's settings file: the look (theme and fonts) and the shell.
//!
//! The file is `$HOME/.config/lumen/settings.toml`, beside the policy file.
//! Its syntax is the small part of TOML the settings need: `key = value` on a
//! line, blank lines, and `#` comments on their own line or after a value.
//!
//! ```toml
//! theme = "system"        # system, light, dark, high-contrast, or a theme file
//! mono_font = "bundled"   # bundled, or a font file
//! mono_size = 14          # 8 to 72
//! prose_font = "bundled"
//! prose_size = 16
//! copy_on_select = true   # true or false: copy a finished mouse selection
//! smooth_scroll = true    # true or false: ease the wheel's scrolling
//! shell = "pwsh"          # the program to run when none is named; see `shell`
//! shell_integration = true # add prompt marks to zsh, bash, fish and PowerShell
//! ```
//!
//! A file is a value with a `/` in it: an absolute path, a path under the home
//! directory (`~/...`), or a path relative to the settings file's directory
//! (`./mine.toml`, `themes/mine.toml`). Every key is optional.
//!
//! The parse is strict, as the policy file's is: an unknown key, a key given
//! twice, a bad value or a line that is not `key = value` ignores the whole
//! file with a notice, and the defaults apply. A theme or font file the
//! settings name that cannot be used is left out with a notice, and the
//! default stands in for it alone.
//!
//! The settings are not policy: they change how things look and never what a
//! program may do, so the file has no owner or mode checks. A theme cannot
//! make text unreadable, since a theme file whose text falls under the spec's
//! visibility floor is refused.

use crate::design::{Builtin, FontFile, Fonts, MAX_FONT_PX, MIN_FONT_PX, ThemeChoice};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The most bytes a settings file may hold.
pub const MAX_BYTES: u64 = 4096;

/// The settings file's place under the user's home directory.
pub const RELATIVE_PATH: &str = ".config/lumen/settings.toml";

/// The theme the settings name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ThemeSetting {
    /// Follow the system's light or dark appearance.
    #[default]
    System,
    /// One built-in palette.
    Builtin(Builtin),
    /// A theme file.
    File(PathBuf),
}

/// A face the settings name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FontSetting {
    /// The bundled face.
    #[default]
    Bundled,
    /// A font file.
    File(PathBuf),
}

/// What a settings file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookSettings {
    /// The theme.
    pub theme: ThemeSetting,
    /// The monospace face.
    pub mono_font: FontSetting,
    /// The monospace size in pixels at scale 1.
    pub mono_size: u32,
    /// The proportional face.
    pub prose_font: FontSetting,
    /// The prose size in pixels at scale 1.
    pub prose_size: u32,
    /// Whether a finished mouse selection is copied at once. On by default.
    pub copy_on_select: bool,
    /// Whether a wheel step eases the view instead of jumping it. On by default.
    pub smooth_scroll: bool,
    /// The program to run when none is named on the command line, if set.
    pub shell: Option<String>,
    /// Whether a shell started directly gets the prompt-mark hooks.
    pub shell_integration: bool,
}

impl Default for LookSettings {
    fn default() -> Self {
        let fonts = Fonts::default();
        Self {
            theme: ThemeSetting::System,
            mono_font: FontSetting::Bundled,
            mono_size: fonts.mono_px(),
            prose_font: FontSetting::Bundled,
            prose_size: fonts.prose_px(),
            copy_on_select: true,
            smooth_scroll: true,
            shell: None,
            shell_integration: true,
        }
    }
}

/// The look the settings give, ready to use, and a notice for each file they
/// name that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// The theme choice.
    pub theme: ThemeChoice,
    /// The fonts.
    pub fonts: Fonts,
    /// The `shell` setting, if the file gave one.
    pub shell: Option<String>,
    /// One line for each theme or font file left out.
    pub notices: Vec<String>,
    /// Whether a shell started directly gets the prompt-mark hooks.
    pub shell_integration: bool,
}

impl LookSettings {
    /// Read the theme and font files the settings name. A file that cannot be
    /// used is left out with a notice, and the default stands in for it.
    #[must_use]
    pub fn resolve(&self) -> Look {
        let mut notices = Vec::new();
        let theme = match &self.theme {
            ThemeSetting::System => ThemeChoice::System,
            ThemeSetting::Builtin(b) => ThemeChoice::Fixed(b.palette()),
            ThemeSetting::File(path) => match crate::theme_file::load(path) {
                Ok(palette) => ThemeChoice::Fixed(palette),
                Err(why) => {
                    notices.push(format!(
                        "theme file {} not used ({why}); following the system",
                        path.display()
                    ));
                    ThemeChoice::System
                }
            },
        };
        let mut face = |setting: &FontSetting, which: &str| match setting {
            FontSetting::Bundled => FontFile::Bundled,
            FontSetting::File(path) => FontFile::load(path).unwrap_or_else(|why| {
                notices.push(format!(
                    "{which} font {} not used ({why}); using the bundled face",
                    path.display()
                ));
                FontFile::Bundled
            }),
        };
        let mono = face(&self.mono_font, "mono");
        let prose = face(&self.prose_font, "prose");
        let fonts = Fonts::default()
            .with_mono(mono)
            .with_prose(prose)
            .with_mono_px(self.mono_size)
            .with_prose_px(self.prose_size);
        Look {
            theme,
            fonts,
            shell: self.shell.clone(),
            notices,
            shell_integration: self.shell_integration,
        }
    }
}

/// What loading the settings found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settings {
    /// There is no settings file: every setting is at its default.
    Defaults,
    /// The file was read and its settings apply.
    Loaded {
        /// The file.
        path: PathBuf,
        /// What it says.
        look: LookSettings,
    },
    /// The file exists but was not used, and the defaults apply.
    Ignored {
        /// The file.
        path: PathBuf,
        /// Why not.
        why: String,
    },
}

impl Settings {
    /// The look settings that apply.
    #[must_use]
    pub fn look(&self) -> LookSettings {
        match self {
            Self::Loaded { look, .. } => look.clone(),
            Self::Defaults | Self::Ignored { .. } => LookSettings::default(),
        }
    }

    /// The notice to show when the file was ignored, or `None`.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        let Self::Ignored { path, why } = self else {
            return None;
        };
        Some(format!(
            "settings file {} ignored ({why}); the defaults apply",
            path.display()
        ))
    }
}

/// Where a value with a `/` in it points: an absolute path, a path under
/// `home` for `~/...`, or a path relative to `dir`.
fn file(value: &str, dir: &Path, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    if let Some(rest) = value.strip_prefix("~/") {
        return home
            .map(|h| h.join(rest))
            .ok_or("a ~/ path needs a home directory");
    }
    let path = Path::new(value);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    })
}

/// The value without a comment after it. A quoted value ends at its closing
/// quote, as in a theme file, so a `#` inside a path is kept; a number ends at
/// a `#`.
fn uncommented(value: &str) -> &str {
    if value.starts_with('"') {
        crate::theme_file::uncommented(value)
    } else {
        value.split('#').next().unwrap_or("").trim_end()
    }
}

/// Parse the text of a settings file found in `dir`, with `home` for `~/`
/// paths.
///
/// # Errors
///
/// Returns the line and the reason for the first line that is not a blank
/// line, a comment, or a known key written once with a good value.
#[allow(clippy::too_many_lines, reason = "one arm per settings key")]
pub fn parse(text: &str, dir: &Path, home: Option<&Path>) -> Result<LookSettings, (usize, String)> {
    let mut look = LookSettings::default();
    let mut seen: Vec<&str> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let Some((key, value)) = content.split_once('=') else {
            return Err((line, "expected key = value".to_owned()));
        };
        let (key, value) = (key.trim(), uncommented(value.trim()));
        let quoted = || {
            value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .filter(|v| !v.contains('"'))
                .ok_or_else(|| (line, format!("`{key}` needs a quoted value")))
        };
        let size = || {
            value
                .parse::<u32>()
                .ok()
                .filter(|px| (MIN_FONT_PX..=MAX_FONT_PX).contains(px))
                .ok_or_else(|| {
                    (
                        line,
                        format!(
                            "`{key}` must be a whole number from {MIN_FONT_PX} to {MAX_FONT_PX}"
                        ),
                    )
                })
        };
        let font = || -> Result<FontSetting, (usize, String)> {
            let v = quoted()?;
            if v == "bundled" {
                Ok(FontSetting::Bundled)
            } else if v.contains('/') {
                Ok(FontSetting::File(
                    file(v, dir, home).map_err(|w| (line, w.to_owned()))?,
                ))
            } else {
                Err((
                    line,
                    format!("`{key}` must be \"bundled\" or a path with a /"),
                ))
            }
        };
        match key {
            "theme" => {
                let v = quoted()?;
                look.theme = if v == "system" {
                    ThemeSetting::System
                } else if let Some(b) = Builtin::named(v) {
                    ThemeSetting::Builtin(b)
                } else if v.contains('/') {
                    ThemeSetting::File(file(v, dir, home).map_err(|w| (line, w.to_owned()))?)
                } else {
                    return Err((
                        line,
                        "`theme` must be system, light, dark, high-contrast or a path with a /"
                            .to_owned(),
                    ));
                };
            }
            "mono_font" => look.mono_font = font()?,
            "prose_font" => look.prose_font = font()?,
            "mono_size" => look.mono_size = size()?,
            "prose_size" => look.prose_size = size()?,
            "copy_on_select" => {
                look.copy_on_select = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err((line, format!("`{key}` must be true or false"))),
                };
            }
            "smooth_scroll" => {
                look.smooth_scroll = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err((line, format!("`{key}` must be true or false"))),
                };
            }
            "shell" => {
                let v = quoted()?;
                if v.trim().is_empty() || v.chars().any(char::is_control) {
                    return Err((
                        line,
                        "`shell` must be a program name or path with no control characters"
                            .to_owned(),
                    ));
                }
                look.shell = Some(v.to_owned());
            }
            "shell_integration" => {
                look.shell_integration = match value {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err((line, "`shell_integration` must be true or false".to_owned()));
                    }
                };
            }
            _ => return Err((line, format!("unknown setting `{key}`"))),
        }
        if seen.contains(&key) {
            return Err((line, format!("`{key}` is given twice")));
        }
        seen.push(key);
    }
    Ok(look)
}

/// The user's home directory: `HOME`, or on Windows, where `HOME` is not usually
/// set, the user profile folder. An empty value counts as not set.
#[must_use]
pub fn home_dir(
    home: Option<OsString>,
    user_profile: Option<OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let set = |v: Option<OsString>| v.filter(|h| !h.is_empty()).map(PathBuf::from);
    set(home).or_else(|| if windows { set(user_profile) } else { None })
}

/// Load the settings from `$HOME/.config/lumen/settings.toml` (under the user
/// profile folder on Windows when `HOME` is not set), or the defaults if there
/// is no home directory.
#[must_use]
pub fn load() -> Settings {
    let Some(home) = home_dir(
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
        cfg!(windows),
    ) else {
        return Settings::Defaults;
    };
    load_from(&home.join(RELATIVE_PATH), Some(&home))
}

/// Load the settings from `path`, with `home` for `~/` paths in it.
#[must_use]
pub fn load_from(path: &Path, home: Option<&Path>) -> Settings {
    let ignored = |why: String| Settings::Ignored {
        path: path.to_path_buf(),
        why,
    };
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::Defaults,
        Err(_) => return ignored("it could not be read".to_owned()),
    };
    if !meta.is_file() {
        return ignored("it is not a regular file".to_owned());
    }
    if meta.len() > MAX_BYTES {
        return ignored(format!("it is larger than {MAX_BYTES} bytes"));
    }
    let Ok(bytes) = std::fs::read(path) else {
        return ignored("it could not be read".to_owned());
    };
    if bytes.len() as u64 > MAX_BYTES {
        return ignored(format!("it is larger than {MAX_BYTES} bytes"));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return ignored("it is not UTF-8 text".to_owned());
    };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    match parse(&text, dir, home) {
        Ok(look) => Settings::Loaded {
            path: path.to_path_buf(),
            look,
        },
        Err((line, why)) => ignored(format!("line {line}: {why}")),
    }
}
