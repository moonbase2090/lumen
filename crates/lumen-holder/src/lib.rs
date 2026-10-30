//! The key-holder process.
//!
//! Each sealed layer has one holder, a small separate process that will hold
//! the layer's key and that never forks or execs. The terminal starts it with
//! exactly four descriptors at fixed numbers ([`layout`]): the seal channel,
//! the spawn socketpair, the EOF pipe and a pidfd for the shell. Everything
//! else is closed before the holder does anything else.
//!
//! This is the process shell: how it is started, hardened and ended, and the
//! one thing it does. The key arrives over the spawn socketpair. The shell asks
//! over the seal channel for command-structure frames to be sealed, and the
//! holder answers with the sealed frame, which the shell writes to the terminal;
//! see [`protocol`] and [`layer`] for what it will and will not seal.
//!
//! The holder ends, and drops (zeroizes) its key, when the terminal's EOF pipe
//! closes, when the shell's pidfd becomes readable, or when the seal channel
//! closes. It is Linux only: elsewhere there is no holder, because shell
//! integration is not sealed there.

#![deny(unsafe_code)]

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "FFI wrappers for prctl, seccomp, mlock, poll and close; see the module docs"
)]
pub mod ffi_linux;
pub mod layer;
pub mod layout;
pub mod principal;
pub mod protocol;
#[cfg(target_os = "linux")]
pub mod run;
#[cfg(target_os = "linux")]
pub mod spawn;

pub use layout::{EOF_FD, PIDFD_FD, SEAL_FD, SPAWN_FD};
