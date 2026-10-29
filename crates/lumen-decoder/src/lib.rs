//! Out-of-process image decoder.
//!
//! The worker decodes one image per process under a sandbox profile. The host
//! side starts it, bounds everything it reads and validates the reply before
//! anything is allocated. The contract is `docs/phase-1/decoder-contract.md`.
//!
//! Only the `ffi_*` modules use `unsafe`; they enter the sandbox and signal a
//! process group, which Rust cannot do safely.

#![deny(unsafe_code)]

pub mod decode;
pub mod ffi_alloc;
#[cfg(unix)]
pub mod ffi_process;
#[cfg(unix)]
pub mod ffi_sandbox;
pub mod limits;
pub mod protocol;
#[cfg(unix)]
pub mod runner;
pub mod unlock;
