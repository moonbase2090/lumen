//! The terminal's side of a sealed layer: start the holder, make the layer's
//! key and deliver it, and get the receiver that reads what the holder seals.
//!
//! The decision to seal is not made here. [`start_layer`] takes the decision
//! the sealing conditions gave and refuses unless it is `Seal`, so a layer that
//! failed a condition never gets a holder or a key. The key is 32 bytes from the
//! system's random source, exists in this process only while it is delivered and
//! while the receiver's keys are derived from it, and is zeroized after both.
//! The terminal does not keep the root: this piece needs only the keys for
//! frames from the shell to the terminal, and there is no terminal-to-shell
//! sealed frame yet.

use crate::spawn::{self, Channels, Running, Started};
use lumen_seal::Decision;
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver};
use std::io::{self, Read as _};
use std::os::fd::OwnedFd;
use std::path::Path;
use zeroize::Zeroizing;

/// A fresh 32-byte root from the system's random source.
fn fresh_root() -> io::Result<Zeroizing<[u8; 32]>> {
    let mut root = Zeroizing::new([0u8; 32]);
    std::fs::File::open("/dev/urandom")?.read_exact(root.as_mut())?;
    Ok(root)
}

/// Start the holder for the shell `shell_pid` (whose pidfd the terminal holds),
/// deliver a fresh key for layer `layer`, and return the holder's channels and
/// the receiver for frames the holder seals to the terminal.
///
/// `terminal` is the pid of the calling process and the call must come from a
/// thread that lives as long as the terminal (see [`spawn::start`]). The
/// returned [`Started::seal`] is the shell's end of the seal channel; handing it
/// to the shell is the shell integration's job and is not done here.
///
/// # Errors
///
/// `PermissionDenied` if `decision` is not `Seal`, with the failed check in the
/// message and no process started. Otherwise the error from starting the holder,
/// reading the random source or delivering the key; a holder that was started
/// is ended before the error is returned.
pub fn start_layer(
    decision: &Decision,
    program: &Path,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    layer: u32,
) -> io::Result<(Started, Receiver)> {
    start_layer_with(
        decision,
        program,
        shell_pidfd,
        shell_pid,
        terminal,
        layer,
        &fresh_root,
    )
}

/// [`start_layer`] with the source of the root given, so a test can show that a
/// refused layer never asks for one. Production uses the system's random source.
///
/// # Errors
///
/// As [`start_layer`].
pub fn start_layer_with(
    decision: &Decision,
    program: &Path,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    layer: u32,
    make_root: &dyn Fn() -> io::Result<Zeroizing<[u8; 32]>>,
) -> io::Result<(Started, Receiver)> {
    if let Decision::GridOnly(reason) = decision {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the layer is grid-only: {reason:?}"),
        ));
    }
    let root = make_root()?;
    let started = spawn::start(program, shell_pidfd, shell_pid, terminal)?;
    // Dropping `started` on an error closes the EOF pipe, which ends the holder.
    spawn::deliver_key(&started, 1, layer, &root)?;
    let keys = DirectionKeys::derive(
        &LayerRoot::new(*root),
        LayerId(layer),
        Direction::ToTerminal,
    );
    let receiver = Receiver::new(keys, LayerId(layer), Direction::ToTerminal);
    Ok((started, receiver))
}

/// [`start_layer`] for a shell that was started with its end of the seal
/// channel: the channels were made first ([`Channels::new`]), the shell took
/// its end ([`Channels::take_shell_end`]), and its pid and pidfd exist now.
///
/// The decision is checked before anything else. On `GridOnly` no root is made,
/// no process is started, and `channels` is dropped, which closes the holder's
/// ends so the shell sees its channel close.
///
/// # Errors
///
/// As [`start_layer`].
pub fn start_layer_on(
    decision: &Decision,
    program: &Path,
    channels: Channels,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    layer: u32,
) -> io::Result<(Running, Receiver)> {
    start_layer_on_with(
        decision,
        program,
        channels,
        shell_pidfd,
        shell_pid,
        terminal,
        layer,
        &fresh_root,
    )
}

/// [`start_layer_on`] with the source of the root given, for tests.
///
/// # Errors
///
/// As [`start_layer`].
#[allow(
    clippy::too_many_arguments,
    reason = "the same inputs as start_layer_with, plus the channels"
)]
pub fn start_layer_on_with(
    decision: &Decision,
    program: &Path,
    channels: Channels,
    shell_pidfd: &OwnedFd,
    shell_pid: u32,
    terminal: u32,
    layer: u32,
    make_root: &dyn Fn() -> io::Result<Zeroizing<[u8; 32]>>,
) -> io::Result<(Running, Receiver)> {
    if let Decision::GridOnly(reason) = decision {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the layer is grid-only: {reason:?}"),
        ));
    }
    let root = make_root()?;
    let running = spawn::start_on(channels, program, shell_pidfd, shell_pid, terminal, &[])?;
    // Dropping `running` on an error closes the EOF pipe, which ends the holder.
    spawn::deliver_key_on(&running.spawn, 1, layer, &root)?;
    let keys = DirectionKeys::derive(
        &LayerRoot::new(*root),
        LayerId(layer),
        Direction::ToTerminal,
    );
    let receiver = Receiver::new(keys, LayerId(layer), Direction::ToTerminal);
    Ok((running, receiver))
}
