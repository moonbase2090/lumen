//! Sealed sessions for the reader screenshot mode (Linux only).
//!
//! A session prompt is a list of command lines, not a byte stream. Each arm's
//! frame comes from the terminal itself: a listed zsh started directly with a
//! sealed layer, the lines typed into the input box one at a time, and the
//! window drawn by [`App::view`] once the shell is back at its prompt. The
//! command boundaries and exit statuses in the margin are the ones the shell's
//! holder sealed. The plain-VT frame runs the same lines in the same shell on
//! the pass-through layer with no sealing.
//!
//! Every wait is on something the shell did: a sealed prompt and the exit
//! status of the line on the arms, the prompt-end mark on the control. On the
//! control one more thing is the terminal's own: with no sealed frames, the
//! box takes keys again a short settle time after its Enter, unless it saw the
//! shell leave the prompt. Once the shell is back and only that time is left,
//! the wait steps by [`SETTLE_STEP`] until the box takes keys. The one
//! deadline only bounds a shell that hangs.

use lumen_term::input_line::Mode;
use lumen_term::marks::Mark;
use lumen_term::seal::{Config, Status};
use lumen_term::{App, Block, Frame, Key, Layer, Mods, Passthrough};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a session may take before it is given up as hung.
const HUNG: Duration = Duration::from_secs(30);

/// How far the control's wait steps while only the box's own settle is left.
const SETTLE_STEP: Duration = Duration::from_millis(50);

/// What a session needs: the sealing settings, with the holder, the listed
/// shells, the system facts and the shell's environment, and the shell.
pub struct Sessions {
    /// The sealing settings. The environment should give the shell its own
    /// `HOME` and `ZDOTDIR` and a fixed `PS1`, so no startup file of the
    /// machine's is read.
    pub config: Config,
    /// The listed shell to run, by absolute path.
    pub shell: String,
    /// The prompt the shell draws, as given in its `PS1`; the frame is taken
    /// once it is drawn again after the last line.
    pub prompt: String,
    /// The shell's home, made afresh for each run so that no run sees what
    /// another left.
    pub home: PathBuf,
}

/// What one run showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The window as the terminal drew it.
    pub frame: Frame,
    /// On a sealed layer, each line the box sent with the exit status its
    /// sealed command end gave. Empty on the control, which records nothing.
    pub typed: Vec<(String, Option<i32>)>,
}

/// The prompt every session's shell draws.
pub const PROMPT: &str = "% ";

impl Sessions {
    /// Sessions in the home directory `home`, made afresh with startup files
    /// that set [`PROMPT`] and keep the machine's own files out, run by the
    /// first of `config.listed` that exists. The shell's `HOME`, `ZDOTDIR` and
    /// `PS1` are added to `config.environment`.
    ///
    /// # Errors
    ///
    /// Returns why the home could not be made, or that no listed shell exists.
    pub fn in_home(home: &Path, mut config: Config) -> Result<Self, String> {
        let shell = config
            .listed
            .iter()
            .find(|p| Path::new(p).is_file())
            .cloned()
            .ok_or("no listed shell is installed")?;
        make_home(home)?;
        let home_text = home.display().to_string();
        config
            .environment
            .insert("HOME".to_owned(), home_text.clone());
        config.environment.insert("ZDOTDIR".to_owned(), home_text);
        config
            .environment
            .insert("PS1".to_owned(), PROMPT.to_owned());
        Ok(Self {
            config,
            shell,
            prompt: PROMPT.to_owned(),
            home: home.to_path_buf(),
        })
    }
}

/// An empty `home` with the startup files a session's shell reads.
fn make_home(home: &Path) -> Result<(), String> {
    if home.exists() {
        std::fs::remove_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
    }
    std::fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
    for (name, text) in [
        (".zshenv", "setopt no_global_rcs\n".to_owned()),
        (".zshrc", format!("PS1='{PROMPT}'\n")),
    ] {
        let file = home.join(name);
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// Run `lines` on a sealed layer `layer`, or on the pass-through layer with no
/// sealing when `layer` is `None`, in a window of `size` pixels at `scale`,
/// and return the window as the terminal draws it, with the lines it recorded.
///
/// # Errors
///
/// Returns why the shell could not start, an arm's layer that did not come up
/// sealed or lost its keys, or a shell that did not come back to its prompt.
pub fn run(
    layer: Option<Box<dyn Layer + Send>>,
    lines: &[String],
    scale: u32,
    size: (u32, u32),
    sessions: &Sessions,
) -> Result<Ran, String> {
    make_home(&sessions.home)?;
    let sealed = layer.is_some();
    let mut app = if let Some(layer) = layer {
        App::start_with_seal(
            layer,
            &sessions.shell,
            &[] as &[&str],
            (scale, size.0, size.1),
            &sessions.config,
        )
    } else {
        // A shell that is not listed is never sealed: it runs on the
        // pass-through layer with the prompt marks only.
        let config = &sessions.config;
        let plain = Config {
            holder: config.holder.clone(),
            listed: Vec::new(),
            probe: Arc::clone(&config.probe),
            sealer: Arc::clone(&config.sealer),
            environment: config.environment.clone(),
        };
        App::start_with_seal(
            Box::new(Passthrough),
            &sessions.shell,
            &[] as &[&str],
            (scale, size.0, size.1),
            &plain,
        )
    }
    .map_err(|e| format!("the shell did not start: {e}"))?;
    // The cursor does not blink, so the frame does not depend on when it is
    // taken.
    app.set_reduce_motion(true);
    let end = Instant::now() + HUNG;
    if sealed && app.session().seal_status() != Status::Sealed {
        return Err(format!(
            "the layer is not sealed: {:?}",
            app.session().seal_status()
        ));
    }
    wait(&mut app, end, "the first prompt", |a| {
        at_prompt(a, sealed, 0, &sessions.prompt)
    })?;
    for (n, line) in lines.iter().enumerate() {
        for c in line.chars() {
            key(&mut app, Key::Char(c))?;
        }
        key(&mut app, Key::Enter)?;
        wait(&mut app, end, line, |a| {
            at_prompt(a, sealed, n + 1, &sessions.prompt)
        })?;
    }
    if sealed && app.session().seal_status() != Status::Sealed {
        return Err(format!(
            "the layer lost its keys: {:?}",
            app.session().seal_status()
        ));
    }
    let typed = app
        .typed_records()
        .into_iter()
        .map(|r| (r.typed, r.status))
        .collect();
    Ok(Ran {
        frame: app.view(),
        typed,
    })
}

fn key(app: &mut App, key: Key) -> Result<(), String> {
    app.key(key, Mods::default())
        .map_err(|e| format!("a key was not sent: {e}"))
}

/// The shell is back at its prompt after `done` lines, with the prompt drawn
/// and the box taking keys. On a sealed layer the box has recorded every line
/// with the exit status its sealed command end gave; on the control the shell
/// has marked the end of `done + 1` prompts.
fn at_prompt(app: &mut App, sealed: bool, done: usize, prompt: &str) -> Progress {
    let shell_says = if sealed {
        let records = app.typed_records();
        records.len() == done && records.iter().all(|r| r.status.is_some())
    } else {
        let ends = app
            .session()
            .marks()
            .iter()
            .filter(|m| **m == Mark::PromptEnd)
            .count();
        ends == done + 1
    };
    if !(shell_says && prompt_drawn(app, prompt)) {
        Progress::Shell
    } else if app.input_mode() == Mode::Editing {
        Progress::Done
    } else {
        Progress::Settling
    }
}

/// How far a wait has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    /// The shell has more to do.
    Shell,
    /// The shell is back at its prompt; the box has not yet taken keys again.
    Settling,
    /// The shell is back and the box takes keys.
    Done,
}

/// The cursor's row of the live grid is the prompt and nothing after it.
fn prompt_drawn(app: &App, prompt: &str) -> bool {
    let Some(Block::Grid(grid)) = app.session().document().blocks().last() else {
        return false;
    };
    let row = grid.cursor().row;
    grid.text_rows()
        .get(row)
        .is_some_and(|text| text == prompt.trim_end())
}

fn wait(
    app: &mut App,
    end: Instant,
    what: &str,
    progress: impl Fn(&mut App) -> Progress,
) -> Result<(), String> {
    loop {
        let now = progress(app);
        if now == Progress::Done {
            return Ok(());
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!(
                "the shell did not come back to its prompt after {what:?}"
            ));
        }
        let step = if now == Progress::Settling {
            left.min(SETTLE_STEP)
        } else {
            left
        };
        app.pump_wait(step);
    }
}
