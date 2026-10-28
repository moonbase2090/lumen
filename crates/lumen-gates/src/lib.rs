//! Phase 0.5 OS gate experiments.
//!
//! Each experiment is a small program that prints what the operating system
//! actually did, so a run on a clean machine is the evidence. Only the
//! `ffi_mem` and `ffi_cred` modules use `unsafe`: they wrap `mmap`, `mlock`,
//! `munlock`, `mincore`, `madvise` and `getrlimit` (G6) and the socket
//! credential calls (G1), which have no safe equivalent in the standard
//! library. Every `unsafe` block there states why it is sound.

#![deny(unsafe_code)]

#[allow(
    unsafe_code,
    reason = "FFI wrappers for memory locking; see the module docs"
)]
pub mod ffi_mem;

#[allow(
    unsafe_code,
    reason = "FFI wrappers for socket credentials; see the module docs"
)]
pub mod ffi_cred;
