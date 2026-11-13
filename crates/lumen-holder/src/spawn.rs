//! The terminal's side: start a holder with its four descriptors.

use crate::ffi_linux;
use crate::layout::ALL;
use crate::principal;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// A running holder and the terminal's ends of its channels.
pub struct Started {
    /// The holder process.
    pub child: Child,
    /// The shell's end of the seal channel, to hand to the shell integration.
    pub seal: OwnedFd,
    /// The terminal's end of the spawn socketpair, where the key is delivered.
    pub spawn: OwnedFd,
    /// The write end of the EOF pipe. Closing it, or dying, ends the holder.
    /// It is close-on-exec and must never be passed to a shell.
    pub eof_write: OwnedFd,
}

/// The channels of one layer, made before the shell exists so that the shell
/// can be started with its end. The terminal then starts the holder with the
/// rest, once the shell's pid and pidfd are known (see [`start_on`]).
///
/// Every descriptor is close-on-exec. Dropping the value without starting a
/// holder closes the holder's ends, so a shell holding its end sees the channel
/// close.
pub struct Channels {
    seal_holder: OwnedFd,
    seal_shell: Option<OwnedFd>,
    spawn_holder: OwnedFd,
    spawn_terminal: OwnedFd,
    eof_read: OwnedFd,
    eof_write: OwnedFd,
}

impl Channels {
    /// Make the seal channel, the spawn socketpair and the EOF pipe.
    ///
    /// # Errors
    ///
    /// Returns why a channel could not be made.
    pub fn new() -> io::Result<Self> {
        let (seal_holder, seal_shell) = ffi_linux::seqpacket_pair()?;
        // Set before the shell can send, so no message arrives without credentials.
        ffi_linux::enable_passcred(seal_holder.as_raw_fd())?;
        let (spawn_holder, spawn_terminal) = ffi_linux::seqpacket_pair()?;
        ffi_linux::enable_passcred(spawn_holder.as_raw_fd())?;
        let (eof_read, eof_write) = ffi_linux::pipe()?;
        Ok(Self {
            seal_holder,
            seal_shell: Some(seal_shell),
            spawn_holder,
            spawn_terminal,
            eof_read,
            eof_write,
        })
    }

    /// The shell's end of the seal channel, once. The terminal maps it into the
    /// shell it spawns and nowhere else.
    pub fn take_shell_end(&mut self) -> Option<OwnedFd> {
        self.seal_shell.take()
    }
}

/// A running holder and the terminal's ends of its channels, when the channels
/// were made first ([`Channels`]).
pub struct Running {
    /// The holder process.
    pub child: Child,
    /// The terminal's end of the spawn socketpair, where the key is delivered.
    pub spawn: OwnedFd,
    /// The write end of the EOF pipe. Closing it, or dying, ends the holder.
    /// It is close-on-exec and must never be passed to a shell.
    pub eof_write: OwnedFd,
}

/// Start `program` as a holder for the shell that `shell_pidfd` names, on
/// channels made first. The shell's end must already have been taken
/// ([`Channels::take_shell_end`]); a holder is not started while the terminal
/// still holds it. See [`start_with`] for the rest.
///
/// # Errors
///
/// Returns why the program could not start, or that the shell has exited.
pub fn start_on(
    channels: Channels,
    program: &Path,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    extra: &[&str],
) -> io::Result<Running> {
    if channels.seal_shell.is_some() {
        return Err(io::Error::other(
            "the shell's end of the seal channel was never given to a shell",
        ));
    }
    // The shell's start time is read while the pidfd still pins its pid.
    let start_time = principal::start_time(shell_pid)
        .ok_or_else(|| io::Error::other("the shell's start time cannot be read"))?;
    if ffi_linux::pidfd_is_readable(shell_pidfd.as_raw_fd())? {
        return Err(io::Error::other("the shell has already exited"));
    }
    let Channels {
        seal_holder,
        spawn_holder,
        spawn_terminal,
        eof_read,
        eof_write,
        ..
    } = channels;
    let sources = [
        seal_holder.as_raw_fd(),
        spawn_holder.as_raw_fd(),
        eof_read.as_raw_fd(),
        shell_pidfd.as_raw_fd(),
    ];
    let mut command = Command::new(program);
    command
        .arg("--terminal")
        .arg(terminal.to_string())
        .arg("--shell-pid")
        .arg(shell_pid.to_string())
        .arg("--shell-start")
        .arg(start_time.to_string())
        .args(extra)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: the closure runs between fork and exec and calls only `fcntl`,
    // `dup2` and `close` through `place_descriptors`, which are async-signal-safe,
    // and it allocates nothing.
    #[allow(unsafe_code, reason = "pre_exec is unsafe; see the safety comment")]
    unsafe {
        command.pre_exec(move || ffi_linux::place_descriptors(&sources, &ALL));
    }
    let child = command.spawn()?;
    Ok(Running {
        child,
        spawn: spawn_terminal,
        eof_write,
    })
}

/// Start `program` as a holder for the shell that `shell_pidfd` names. The
/// holder gets its own copy of the descriptor; the terminal keeps its own.
/// `terminal` is the calling process's pid.
///
/// The holder gets exactly four descriptors, at the numbers in
/// [`crate::layout`]; the environment is empty and its standard streams are
/// closed. Every descriptor the terminal keeps is close-on-exec.
///
/// The holder is killed when the *thread* that called this exits (see
/// [`crate::ffi_linux::die_with_parent`]), so call it from a thread that lives
/// as long as the terminal.
///
/// # Errors
///
/// Returns why a channel could not be made or the program could not start.
pub fn start(
    program: &Path,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
) -> io::Result<Started> {
    start_with(program, shell_pidfd, shell_pid, terminal, &[])
}

/// [`start`] with more arguments for the holder. The holder's standard output
/// is a pipe the terminal can read from `child.stdout`; a holder writes
/// nothing to it except when asked to check its own hardening. The shell's end
/// of the seal channel comes back in [`Started::seal`]: this is for callers
/// that play the shell themselves. A real shell is started with its end first,
/// see [`Channels`] and [`start_on`].
///
/// # Errors
///
/// As [`start`].
pub fn start_with(
    program: &Path,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    extra: &[&str],
) -> io::Result<Started> {
    let mut channels = Channels::new()?;
    let seal = channels
        .take_shell_end()
        .ok_or_else(|| io::Error::other("no shell end"))?;
    let running = start_on(channels, program, shell_pidfd, shell_pid, terminal, extra)?;
    Ok(Started {
        child: running.child,
        seal,
        spawn: running.spawn,
        eof_write: running.eof_write,
    })
}

/// Deliver the layer's key to the holder over the spawn socketpair: one
/// datagram, the only thing that ever travels on it. The message buffer is
/// zeroized as soon as it is sent. The holder takes one key for its whole life.
///
/// # Errors
///
/// Returns the error from the write.
pub fn deliver_key(started: &Started, number: u32, layer: u32, root: &[u8; 32]) -> io::Result<()> {
    deliver_key_on(&started.spawn, number, layer, root)
}

/// [`deliver_key`] on the terminal's end of the spawn socketpair itself.
///
/// # Errors
///
/// Returns the error from the write.
pub fn deliver_key_on(spawn: &OwnedFd, number: u32, layer: u32, root: &[u8; 32]) -> io::Result<()> {
    use std::io::Write as _;
    let message = crate::protocol::key_message(number, layer, root);
    let mut end = std::fs::File::from(spawn.try_clone()?);
    end.write_all(&message[..])
}
