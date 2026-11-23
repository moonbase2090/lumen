//! The windows restored on the next launch.
//!
//! This is application state, not a user setting. It is a small bounded list
//! beside the settings file. Each line is `x y width height zoom`; `- -` means
//! the platform did not expose a position, and `zoom` is the window's zoom
//! level, 1 (the display's own scale) to 4 (see [`crate::actions::zoom_level`]).
//! A line of the first four fields alone, as earlier versions wrote, is a
//! window at zoom level 1. A bad file is ignored whole.

use crate::actions::MAX_ZOOM_LEVEL;
use std::io::Read;
use std::path::{Path, PathBuf};

const RELATIVE_PATH: &str = ".config/lumen/windows";
const MAX_BYTES: u64 = 4096;
const MAX_WINDOWS: usize = 32;
const MAX_DIMENSION: u32 = 16_384;

/// One window's outer position and inner size, in physical pixels, and its
/// zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub position: Option<(i32, i32)>,
    pub size: (u32, u32),
    pub zoom: u32,
}

/// The state file and what it restored.
pub struct Store {
    path: Option<PathBuf>,
    restored: Vec<Placement>,
    notice: Option<String>,
}

impl Store {
    /// Read the state under the user's home directory.
    pub fn system() -> Self {
        let home = crate::settings::home_dir(
            std::env::var_os("HOME"),
            std::env::var_os("USERPROFILE"),
            cfg!(windows),
        );
        home.map_or_else(
            || Self {
                path: None,
                restored: Vec::new(),
                notice: None,
            },
            |home| Self::from_path(home.join(RELATIVE_PATH)),
        )
    }

    fn from_path(path: PathBuf) -> Self {
        let (restored, notice) = match read(&path) {
            Ok(placements) if placements.is_empty() => (Vec::new(), None),
            Ok(placements) => (placements, None),
            Err(ReadError::Missing) => (Vec::new(), None),
            Err(why) => (
                Vec::new(),
                Some(format!(
                    "window state {} ignored ({why}); opening one window",
                    path.display()
                )),
            ),
        };
        Self {
            path: Some(path),
            restored,
            notice,
        }
    }

    /// The placements to open, consumed once when the application resumes.
    pub fn take_restored(&mut self) -> Vec<Placement> {
        std::mem::take(&mut self.restored)
    }

    /// Why the state was ignored, if it was.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Replace the state with the windows that are open now.
    pub fn save(&self, placements: &[Placement]) -> Result<(), String> {
        if placements.is_empty() {
            // Closing the last window is still a placement to restore. An
            // empty file means no saved state, so keep the last nonempty one.
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let Some(parent) = path.parent() else {
            return Err("window state path has no parent directory".to_owned());
        };
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        std::fs::write(path, encode(placements)).map_err(|e| e.to_string())
    }
}

#[derive(Debug)]
enum ReadError {
    Missing,
    Refused(&'static str),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("missing"),
            Self::Refused(why) => f.write_str(why),
        }
    }
}

fn read(path: &Path) -> Result<Vec<Placement>, ReadError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ReadError::Missing),
        Err(_) => return Err(ReadError::Refused("not readable")),
    };
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| ReadError::Refused("not text"))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err(ReadError::Refused("too large"));
    }
    parse(&text).map_err(ReadError::Refused)
}

fn parse(text: &str) -> Result<Vec<Placement>, &'static str> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if out.len() == MAX_WINDOWS {
            return Err("too many windows");
        }
        let fields: Vec<&str> = line.split_ascii_whitespace().collect();
        let (x, y, width, height, zoom) = match fields.as_slice() {
            [x, y, width, height] => (x, y, width, height, None),
            [x, y, width, height, zoom] => (x, y, width, height, Some(zoom)),
            _ => return Err("bad line"),
        };
        let zoom = match zoom {
            None => 1,
            Some(zoom) => zoom
                .parse()
                .ok()
                .filter(|z| (1..=MAX_ZOOM_LEVEL).contains(z))
                .ok_or("bad zoom")?,
        };
        let position = match (*x, *y) {
            ("-", "-") => None,
            ("-", _) | (_, "-") => return Err("bad position"),
            (x, y) => Some((
                x.parse().map_err(|_| "bad position")?,
                y.parse().map_err(|_| "bad position")?,
            )),
        };
        let width = width.parse().map_err(|_| "bad size")?;
        let height = height.parse().map_err(|_| "bad size")?;
        if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
            return Err("bad size");
        }
        out.push(Placement {
            position,
            size: (width, height),
            zoom,
        });
    }
    Ok(out)
}

fn encode(placements: &[Placement]) -> String {
    let mut out = String::new();
    for placement in placements.iter().take(MAX_WINDOWS) {
        let (x, y) = placement.position.map_or_else(
            || ("-".to_owned(), "-".to_owned()),
            |(x, y)| (x.to_string(), y.to_string()),
        );
        out.push_str(&x);
        out.push(' ');
        out.push_str(&y);
        out.push(' ');
        out.push_str(&placement.size.0.to_string());
        out.push(' ');
        out.push_str(&placement.size.1.to_string());
        out.push(' ');
        out.push_str(&placement.zoom.clamp(1, MAX_ZOOM_LEVEL).to_string());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Placement, ReadError, Store, encode, parse, read};

    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lumen-window-state-{}-{name}", std::process::id()))
    }

    #[test]
    fn placements_round_trip_with_and_without_a_position() {
        let placements = [
            Placement {
                position: Some((-40, 80)),
                size: (900, 700),
                zoom: 3,
            },
            Placement {
                position: None,
                size: (640, 480),
                zoom: 1,
            },
        ];
        assert_eq!(parse(&encode(&placements)), Ok(placements.to_vec()));
    }

    #[test]
    fn a_state_written_before_zoom_was_kept_reads_at_zoom_level_1() {
        assert_eq!(
            parse("10 20 800 600\n- - 640 480 4\n"),
            Ok(vec![
                Placement {
                    position: Some((10, 20)),
                    size: (800, 600),
                    zoom: 1,
                },
                Placement {
                    position: None,
                    size: (640, 480),
                    zoom: 4,
                },
            ])
        );
    }

    #[test]
    fn a_zoom_outside_1_to_4_refuses_the_whole_state() {
        for bad in ["0", "5", "x", "-1", "2.5"] {
            assert_eq!(
                parse(&format!("0 0 800 600 2\n0 0 800 600 {bad}\n")),
                Err("bad zoom"),
                "{bad}"
            );
        }
    }

    #[test]
    fn one_bad_value_refuses_the_whole_state() {
        assert_eq!(parse("0 0 800 600\n- 2 640 480\n"), Err("bad position"));
        assert_eq!(parse("0 0 0 600\n"), Err("bad size"));
        assert_eq!(parse("0 0 800 600 2 extra\n"), Err("bad line"));
    }

    #[test]
    fn closing_the_last_window_keeps_its_placement_for_the_next_launch() {
        let path = path("last-window");
        let store = Store::from_path(path.clone());
        let placement = Placement {
            position: Some((40, 80)),
            size: (900, 700),
            zoom: 2,
        };
        assert_eq!(store.save(&[placement]), Ok(()));
        assert_eq!(store.save(&[]), Ok(()));
        assert!(matches!(read(&path), Ok(got) if got == vec![placement]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn state_over_4096_bytes_is_refused() {
        let path = path("too-large");
        assert!(std::fs::write(&path, vec![b'x'; 4097]).is_ok());
        assert!(matches!(read(&path), Err(ReadError::Refused("too large"))));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reading_more_than_32_windows_is_refused() {
        assert_eq!(parse(&"0 0 800 600\n".repeat(33)), Err("too many windows"));
    }

    #[test]
    fn a_dimension_over_16384_is_refused() {
        assert_eq!(parse("0 0 16385 600\n"), Err("bad size"));
        assert!(parse("0 0 16384 16384\n").is_ok());
    }

    #[test]
    fn encoding_keeps_at_most_32_windows() {
        let placement = Placement {
            position: None,
            size: (800, 600),
            zoom: 1,
        };
        assert_eq!(encode(&vec![placement; 33]).lines().count(), 32);
    }
}
