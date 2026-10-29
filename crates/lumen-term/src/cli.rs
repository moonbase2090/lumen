//! The command line of the `lumen-term` binary.

use std::time::Duration;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Render scale: 1, or 2 on a high-density screen.
    pub scale: Option<u32>,
    /// Window or snapshot size in pixels.
    pub size: (u32, u32),
    /// Write a PNG of the view to this path instead of opening a window.
    pub snapshot: Option<String>,
    /// How long a snapshot lets the program run before it is taken.
    pub wait: Duration,
    /// Close the window after this long (for smoke tests).
    pub exit_after: Option<Duration>,
    /// What the user allowed for notifications; everything is off by default.
    pub notifications: crate::notify::Settings,
    /// The user's statement that the program is a local one (see `app::place_of`).
    pub says_local: bool,
    /// The program and its arguments; empty means the user's shell.
    pub command: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scale: None,
            size: (800, 600),
            snapshot: None,
            wait: Duration::from_millis(1500),
            exit_after: None,
            notifications: crate::notify::Settings {
                allow_unsigned: false,
                os_notifications: false,
                os_text: false,
            },
            says_local: false,
            command: Vec::new(),
        }
    }
}

/// The usage text.
pub const USAGE: &str = "usage: lumen-term [--scale N] [--size WxH] [--snapshot FILE [--wait MS]] [--exit-after MS] [--allow-unsigned] [--os-notifications [--os-text]] [--local] [--] [PROGRAM [ARGS...]]";

fn number(flag: &str, value: Option<&String>) -> Result<u64, String> {
    value
        .ok_or_else(|| format!("{flag} needs a value"))?
        .parse()
        .map_err(|_| format!("{flag} needs a whole number"))
}

impl Options {
    /// Parse the arguments after the program name.
    ///
    /// # Errors
    ///
    /// Returns a message for an unknown flag, a missing or bad value, a size
    /// that is not `WxH` with both at least 1, or a scale that is not 1 to 4.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut at = 0;
        while at < args.len() {
            let arg = args[at].as_str();
            match arg {
                "--" => {
                    options.command = args[at + 1..].to_vec();
                    return Ok(options);
                }
                "--scale" => {
                    let n = number(arg, args.get(at + 1))?;
                    if !(1..=4).contains(&n) {
                        return Err("--scale must be 1 to 4".into());
                    }
                    options.scale = u32::try_from(n).ok();
                    at += 1;
                }
                "--size" => {
                    let value = args.get(at + 1).ok_or("--size needs a value")?;
                    let (wide, high) = value.split_once('x').ok_or("--size needs WxH")?;
                    let (wide, high): (u32, u32) = (
                        wide.parse().map_err(|_| "--size needs WxH")?,
                        high.parse().map_err(|_| "--size needs WxH")?,
                    );
                    if wide == 0 || high == 0 {
                        return Err("--size needs both sides at least 1".into());
                    }
                    options.size = (wide, high);
                    at += 1;
                }
                "--allow-unsigned" => options.notifications.allow_unsigned = true,
                "--os-notifications" => options.notifications.os_notifications = true,
                "--os-text" => options.notifications.os_text = true,
                "--local" => options.says_local = true,
                "--snapshot" => {
                    options.snapshot =
                        Some(args.get(at + 1).ok_or("--snapshot needs a path")?.clone());
                    at += 1;
                }
                "--wait" => {
                    options.wait = Duration::from_millis(number(arg, args.get(at + 1))?);
                    at += 1;
                }
                "--exit-after" => {
                    options.exit_after =
                        Some(Duration::from_millis(number(arg, args.get(at + 1))?));
                    at += 1;
                }
                flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
                _ => {
                    options.command = args[at..].to_vec();
                    return Ok(options);
                }
            }
            at += 1;
        }
        if options.notifications.os_text && !options.notifications.os_notifications {
            return Err("--os-text needs --os-notifications".into());
        }
        Ok(options)
    }
}
