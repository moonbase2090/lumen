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

use crate::spawn::{self, Started};
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
