//! The operating system's notification, behind [`OsSink`].
//!
//! The notifier decides whether an OS notification is allowed at all (the
//! setting is off by default, and the conditions of the Notifications rules
//! hold); this only shows one. It starts a fixed system program with the text
//! as an argument or an environment variable, never inside a script or a
//! shell command line, so what the program wrote cannot become a command. The
//! text has already been cleaned and escaped for markup by the notifier.
//!
//! There is no way back from the notification: no action, no reply, no click
//! handler, so the token a notification carries is not passed on and nothing
//! the operating system reports about it can reach the terminal or a program.
//! A notification that cannot be shown (the program is missing, or the launch
//! fails) is dropped and counted; it is not retried and never becomes a toast
//! with different text.

use crate::notify::{OsNotification, OsSink};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The title of every notification: fixed, from the terminal.
pub const TITLE: &str = "Lumen terminal";

/// The most launches in flight at once. The notifier's own budget is far
/// below this; it bounds the processes if the clock or the budget is wrong.
pub const MAX_IN_FLIGHT: usize = 4;

/// An operating system with a notification path here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS.
    MacOs,
    /// A Linux desktop with a notification daemon.
    Linux,
    /// Windows.
    Windows,
}

impl Platform {
    /// The platform this build runs on, or `None` where there is no backend.
    #[must_use]
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "macos" => Some(Self::MacOs),
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

/// A program to start: a fixed absolute path, its arguments, and environment
/// variables to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program, by absolute path.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Variables added to its environment.
    pub env: Vec<(String, String)>,
}

/// The variable that carries the text on Windows.
pub const WINDOWS_TEXT_VARIABLE: &str = "LUMEN_NOTIFICATION_TEXT";

/// The macOS script. The text is `item 1 of argv`, an argument, so no part of
/// it is ever read as script.
const MACOS_SCRIPT: [&str; 3] = [
    "on run argv",
    "display notification (item 2 of argv) with title (item 1 of argv)",
    "end run",
];

/// The Windows script. It is constant: the text arrives in the environment
/// and becomes a DOM text node, never script or XML source.
const WINDOWS_SCRIPT: &str = "[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime]; \
[void][Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType=WindowsRuntime]; \
$x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText01); \
$t = $x.GetElementsByTagName('text').Item(0); \
[void]$t.AppendChild($x.CreateTextNode($env:LUMEN_NOTIFICATION_TEXT)); \
$toast = [Windows.UI.Notifications.ToastNotification]::new($x); \
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Lumen').Show($toast)";

/// Undo [`crate::notify::escape_markup`], for an API that shows plain text.
/// Only the five entities the escape writes are read; anything else stays as it is.
#[must_use]
pub fn unescape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let (decoded, len) = [
            ("&amp;", '&'),
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&#39;", '\''),
        ]
        .iter()
        .find(|(entity, _)| rest.starts_with(entity))
        .map_or(('&', 1), |(entity, c)| (*c, entity.len()));
        out.push(decoded);
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

/// How to show `note` on `platform`.
#[must_use]
pub fn invocation(platform: Platform, note: &OsNotification) -> Invocation {
    match platform {
        Platform::MacOs => {
            let mut args = Vec::new();
            for line in MACOS_SCRIPT {
                args.push(OsString::from("-e"));
                args.push(OsString::from(line));
            }
            args.push(OsString::from(TITLE));
            args.push(OsString::from(unescape_markup(&note.text)));
            Invocation {
                program: PathBuf::from("/usr/bin/osascript"),
                args,
                env: Vec::new(),
            }
        }
        Platform::Linux => Invocation {
            program: PathBuf::from("/usr/bin/notify-send"),
            args: vec![
                OsString::from("--app-name=Lumen"),
                OsString::from("--"),
                OsString::from(TITLE),
                OsString::from(&note.text),
            ],
            env: Vec::new(),
        },
        Platform::Windows => Invocation {
            program: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            args: vec![
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from(WINDOWS_SCRIPT),
            ],
            env: vec![(
                WINDOWS_TEXT_VARIABLE.to_owned(),
                unescape_markup(&note.text),
            )],
        },
    }
}

/// Starts a program. The real one starts a process; a test records.
pub trait Launch {
    /// Start `invocation` and return at once. `Err` says it could not be started.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    fn launch(&mut self, invocation: &Invocation) -> Result<(), String>;
}

/// Starts a real process with no shell, no standard input and no output, and
/// reaps it on a thread of its own so the caller never waits.
#[derive(Debug, Default)]
pub struct Process {
    in_flight: Arc<AtomicUsize>,
}

impl Launch for Process {
    fn launch(&mut self, invocation: &Invocation) -> Result<(), String> {
        if self.in_flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT {
            return Err("too many notification programs running".to_owned());
        }
        let mut child = Command::new(&invocation.program)
            .args(&invocation.args)
            .envs(invocation.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", invocation.program.display()))?;
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let counter = Arc::clone(&self.in_flight);
        std::thread::spawn(move || {
            let _ = child.wait();
            counter.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(())
    }
}

/// An [`OsSink`] that shows the notification with the platform's own program.
#[derive(Debug)]
pub struct SystemSink<L: Launch = Process> {
    platform: Option<Platform>,
    launcher: L,
    shown: usize,
    failed: usize,
}

impl SystemSink<Process> {
    /// A sink for this platform, or one that drops everything where there is no backend.
    #[must_use]
    pub fn new() -> Self {
        Self::with(Platform::current(), Process::default())
    }
}

impl Default for SystemSink<Process> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: Launch> SystemSink<L> {
    /// A sink for `platform` that starts programs with `launcher`.
    #[must_use]
    pub fn with(platform: Option<Platform>, launcher: L) -> Self {
        Self {
            platform,
            launcher,
            shown: 0,
            failed: 0,
        }
    }

    /// Notifications handed to the operating system.
    #[must_use]
    pub fn shown(&self) -> usize {
        self.shown
    }

    /// Notifications dropped because there is no backend or the launch failed.
    #[must_use]
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// The launcher, for a test to read.
    #[must_use]
    pub fn launcher(&self) -> &L {
        &self.launcher
    }
}

impl<L: Launch> OsSink for SystemSink<L> {
    fn send(&mut self, notification: &OsNotification) {
        let Some(platform) = self.platform else {
            self.failed += 1;
            return;
        };
        match self.launcher.launch(&invocation(platform, notification)) {
            Ok(()) => self.shown += 1,
            Err(_) => self.failed += 1,
        }
    }
}
