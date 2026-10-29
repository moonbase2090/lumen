//! When the terminal may seal shell integration.
//!
//! Shell integration is sealed only on Linux, and only when every Linux
//! sealing condition holds at the layer's start and keeps holding at every
//! command start. Otherwise the layer is grid-only: no holder, no keys, no
//! seal descriptor. This crate decides which, and says which check failed so
//! the terminal can show a notice that names it.
//!
//! [`conditions`] is the decision, over a [`Probe`] that reports what the
//! system says. [`probe::FsProbe`] is the Linux probe: it reads `/proc` and
//! `/proc/sys` (under a configurable root, so tests use a made-up tree), and
//! the two checks the file system cannot answer go through the one FFI module,
//! `ffi_linux`. Every read that fails is a failed check.

#![deny(unsafe_code)]

pub mod conditions;
#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "FFI wrappers for the namespace-parent ioctl and pidfd; see the module docs"
)]
pub mod ffi_linux;
pub mod probe;

pub use conditions::{Decision, Reason, Sealer, Shell};
pub use probe::{FsProbe, Probe};
