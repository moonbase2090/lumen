//! Tools for the decoder fuzz campaign (the decoder gate).
//!
//! - [`classify`] turns a libFuzzer log into crash records. A hang over 2 s,
//!   memory over 512 MiB, a sanitizer report and a Rust panic all count.
//! - [`identity`] hashes what makes up a build identity.
//! - [`ledger`] builds the ledger `lumen-evidence --decoder` judges from the
//!   records a campaign keeps.
//! - [`miri`] checks the record `scripts/decoder-miri.sh` writes.
//! - [`gate`] says where a format stands against the gate, for the stop rule.
//! - [`slice`] sizes the last slice so the CPU-hour cap holds.
//! - [`ckpt`] writes a block of bytes as console lines and reads the newest
//!   verified one back out of noisy console text.
//! - [`escape`] reads the sandbox escape result from a test run.
//! - The `seeds` example writes seed images made by real encoders.
//!
//! None of this fuzzes. The campaign runs the fuzz targets under
//! `crates/lumen-decoder/fuzz` and feeds its logs and samples here.

#![forbid(unsafe_code)]

pub mod ckpt;
pub mod classify;
pub mod escape;
pub mod gate;
pub mod identity;
pub mod ledger;
pub mod miri;
pub mod slice;
