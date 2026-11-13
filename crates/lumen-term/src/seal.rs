//! Sealed shell integration on Linux: the seal channel, the shell started with
//! its end of it, the sealing decision and the holder.
//!
//! The order is fixed. The channels are made first, the listed shell is spawned
//! directly (no wrapper, no `-c`) with its end of the seal channel at
//! [`SHELL_SEAL_FD`] and a zsh shim that runs before the user's own startup
//! files, then the terminal opens a pidfd for the shell it spawned and asks the
//! sealing conditions for a decision. Only on `Seal` is the holder started and
//! given a fresh key ([`lumen_holder::terminal::start_layer_on`]). On any other
//! decision, or if the holder cannot start, no holder and no key exist, both
//! ends of the channel are closed, and the shell runs as a plain grid shell.
//!
//! Elsewhere there is no holder: macOS and every other system stay grid-only
//! and this module is not built.
//!
//! The shim is a zsh `ZDOTDIR` file ([`ZSHENV`]), the only listed shell. A shell
//! with no shim gets no keys. The shim does not compare the shell's pid with a
//! variable, because the pid does not exist before the spawn and the environment
//! is fixed at spawn: the holder instead refuses any sender whose kernel peer
//! credentials are not the shell's pid and start time, and the descriptor is
//! a nested shell has no descriptor at the fixed number. zsh cannot mark the descriptor close-on-exec, so the shell's children do inherit it; the holder's check keeps them from sealing anything.

use crate::pty::{Pty, SpawnError};
use lumen_holder::ffi_linux::pidfd_open;
use lumen_holder::spawn::{Channels, Running};
use lumen_holder::terminal::start_layer_on;
use lumen_seal::{Decision, FsProbe, Probe, Sealer, Shell};
use lumen_sealed::Receiver;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

/// The descriptor on which the shell has its end of the seal channel. The shim
/// moves it out of the low numbers and closes this one before any user file runs.
pub const SHELL_SEAL_FD: RawFd = 3;

/// The shells that may have a sealed layer, by the absolute path the terminal
/// executes. Only zsh has a shim.
///
/// A program given as a link is resolved first ([`resolve`]) and the resolved
/// path is what is listed and what is executed. On Debian and Ubuntu `zsh` is a
/// link to `zsh5`, and the kernel names the running program `zsh5`, which is the
/// name the conditions compare against.
pub const LISTED: [&str; 5] = [
    "/bin/zsh",
    "/bin/zsh5",
    "/usr/bin/zsh",
    "/usr/bin/zsh5",
    "/usr/local/bin/zsh",
];

/// The layer number of the one layer a terminal-spawned shell has.
const LAYER: u32 = 1;

/// The zsh shim, written to a private directory the shell reads as `ZDOTDIR`.
pub const ZSHENV: &str = include_str!("seal/zshenv");

/// Whether a shell has a sealed layer, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The program is not a listed shell started directly: there is no layer to
    /// seal and nothing to report.
    NotApplicable,
    /// The shell has a holder and the terminal reads what it seals.
    Sealed,
    /// The layer is grid-only. The text names the check that failed.
    GridOnly(String),
}

/// What the sealed start needs from outside: where the holder is, which shells
/// are listed and where the system facts come from. [`Config::default`] is the
/// shipped terminal.
pub struct Config {
    /// The holder executable.
    pub holder: PathBuf,
    /// The listed shells, by absolute path.
    pub listed: Vec<String>,
    /// The system facts the conditions read.
    pub probe: Box<dyn Probe + Send + Sync>,
    /// The terminal-wide state of the checks, which holds the latch. The
    /// shipped terminal has one; a test gives each case its own.
    pub sealer: Arc<Sealer>,
    /// Added to the shell's environment. Empty in the shipped terminal.
    pub environment: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            holder: holder_path(),
            listed: LISTED.iter().map(|s| (*s).to_owned()).collect(),
            probe: Box::new(FsProbe::default()),
            sealer: Arc::clone(sealer()),
            environment: BTreeMap::new(),
        }
    }
}

/// The holder next to the terminal's executable.
#[must_use]
pub fn holder_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("lumen-holder")))
        .unwrap_or_else(|| "lumen-holder".into())
}

/// The terminal-wide state of the checks, which includes the latch: a value
/// below a floor makes every layer of this terminal grid-only until it exits.
fn sealer() -> &'static Arc<Sealer> {
    static SEALER: OnceLock<Arc<Sealer>> = OnceLock::new();
    SEALER.get_or_init(|| Arc::new(Sealer::new()))
}

/// The listed path `program` resolves to, if it is an absolute path to a
/// listed shell. Links are followed here, in the terminal, and the result is
/// what is executed, so the path the conditions see is the path that runs.
#[must_use]
pub fn resolve(config: &Config, program: &str) -> Option<String> {
    if !Path::new(program).is_absolute() {
        return None;
    }
    let resolved = std::fs::canonicalize(program).ok()?;
    let resolved = resolved.to_str()?;
    config
        .listed
        .iter()
        .any(|l| l == resolved)
        .then(|| resolved.to_owned())
}

/// The private directory that holds the shim: owned by the user, mode 0700,
/// created fresh (so a planted path or link is refused) and removed on drop.
struct ShimDir(PathBuf);

impl ShimDir {
    fn create() -> std::io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let mut random = [0u8; 8];
        std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut random)?;
        let mut name = format!(
            "lumen-shim-{}-{}-",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        for byte in random {
            let _ = write!(name, "{byte:02x}");
        }
        let path = base.join(name);
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        let dir = Self(path);
        let file = dir.0.join(".zshenv");
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(file)?;
        std::io::Write::write_all(&mut out, ZSHENV.as_bytes())?;
        Ok(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ShimDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Wait, for a short while, until `pid` is running `path`: until then `/proc`
/// names the terminal's own copy, not the shell, and the conditions would read
/// the wrong `comm`. If it never is, the conditions run anyway and fail closed.
fn wait_for_exec(pid: u32, path: &str) {
    let exe = format!("/proc/{pid}/exe");
    for _ in 0..1000 {
        if std::fs::read_link(&exe).is_ok_and(|p| p == Path::new(path)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// What a session keeps of its sealed layer while it lives: the holder, the
/// terminal's pidfd for the shell and the shim directory. Dropping it ends the
/// holder.
pub struct Sealing {
    running: Option<Running>,
    // Held for the holder's lifetime: the terminal's own pidfd for the shell.
    _pidfd: OwnedFd,
    _shim: ShimDir,
    status: Status,
}

impl Sealing {
    /// Whether the layer is sealed, or why not.
    #[must_use]
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The holder's process id, while there is a holder.
    #[must_use]
    pub fn holder_pid(&self) -> Option<u32> {
        self.running.as_ref().map(|r| r.child.id())
    }
}

impl Drop for Sealing {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            // Closing the EOF pipe ends the holder; the kill is for one that
            // is busy. Reap it so it does not stay a zombie.
            drop(running.eof_write);
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

/// A shell started for a sealed layer: its PTY, what the terminal keeps, and
/// the receiver for what the holder seals (present only if the layer is sealed).
pub struct Started {
    /// The shell on its PTY.
    pub pty: Pty,
    /// What the session keeps.
    pub sealing: Sealing,
    /// The receiver for frames the holder seals, if there is a holder.
    pub receiver: Option<Receiver>,
}

/// Start the listed shell `program` for a sealed layer, as described in the
/// module documentation. The call must come from a thread that lives as long
/// as the terminal: the holder is killed when it exits.
///
/// # Errors
///
/// Returns why the shell could not be started. A sealing decision other than
/// `Seal`, or a holder that cannot start, is not an error: the shell runs, and
/// [`Sealing::status`] says why it is grid-only.
pub fn start(
    columns: usize,
    rows: usize,
    program: &str,
    config: &Config,
) -> Result<Started, SpawnError> {
    let fail = |what: &str, e: std::io::Error| SpawnError(format!("{what}: {e}"));
    let shim = ShimDir::create().map_err(|e| fail("could not make the shell shim", e))?;
    let mut channels = Channels::new().map_err(|e| fail("could not make the seal channel", e))?;
    let seal_end = channels
        .take_shell_end()
        .ok_or_else(|| SpawnError("the seal channel has no shell end".to_owned()))?;
    let mut environment = config.environment.clone();
    // The user's own ZDOTDIR, or empty for none: the shim puts it back.
    let user_zdotdir = environment
        .get("ZDOTDIR")
        .cloned()
        .or_else(|| std::env::var("ZDOTDIR").ok())
        .unwrap_or_default();
    environment.insert("ZDOTDIR".to_owned(), shim.path().display().to_string());
    environment.insert("LUMEN_USER_ZDOTDIR".to_owned(), user_zdotdir);
    let no_arguments: [&str; 0] = [];
    let pty = Pty::spawn_with_seal_channel(
        columns,
        rows,
        program,
        &no_arguments,
        &environment,
        seal_end,
        SHELL_SEAL_FD,
    )?;
    let pid = pty
        .process_id()
        .ok_or_else(|| SpawnError("the shell has no process id".to_owned()))?;
    let pidfd = pidfd_open(pid).map_err(|e| fail("could not open a pidfd for the shell", e))?;
    wait_for_exec(pid, program);
    let shell = Shell {
        pid,
        pidfd: Some(pidfd.as_raw_fd()),
        path: program,
        direct: true,
    };
    let decision = config
        .sealer
        .ready(config.probe.as_ref(), &shell, &listed_refs(config));
    let (running, receiver, status) = match &decision {
        Decision::Seal => match start_layer_on(
            &decision,
            &config.holder,
            channels,
            &pidfd,
            pid,
            std::process::id(),
            LAYER,
        ) {
            Ok((running, receiver)) => (Some(running), Some(receiver), Status::Sealed),
            Err(e) => (
                None,
                None,
                Status::GridOnly(format!("the holder could not be started: {e}")),
            ),
        },
        Decision::GridOnly(reason) => {
            // No holder, no key: closing the channels is all there is to do.
            drop(channels);
            (None, None, Status::GridOnly(reason.to_string()))
        }
    };
    Ok(Started {
        pty,
        sealing: Sealing {
            running,
            _pidfd: pidfd,
            _shim: shim,
            status,
        },
        receiver,
    })
}

fn listed_refs(config: &Config) -> Vec<&str> {
    config.listed.iter().map(String::as_str).collect()
}
