//! The numeric limits and aggregate budgets of the security resource limits
//! table, as code that does the arithmetic once.
//!
//! Nothing in the terminal consumes this crate yet. It is the model the image
//! path, the grid manager and the document store are to ask before they
//! allocate, and the language-neutral vectors in `conformance/vectors/budgets.json`
//! are run against it.
//!
//! - [`limits`] holds every number of the table.
//! - [`Ledger`] is the per-document accounting of the pooled resources, with the
//!   [budget shares](Ledger): a quarter of every budget is kept for local use, no
//!   producer takes more than half of the rest, and unsigned producers share a
//!   quarter between them.
//! - [`RateLimit`], [`ProducerRates`] and [`DocumentRates`] are the per-second
//!   limits, in sliding windows.
//! - [`item`] holds the checks of one item: a frame, a nesting depth, a table, a
//!   grid size, an image.
//!
//! Every reservation is made before the thing it pays for is allocated, and all
//! arithmetic is 64-bit and checked, so an overflow is a refusal, not a wrap.

#![forbid(unsafe_code)]

pub mod item;
mod ledger;
pub mod limits;
mod rate;

pub use ledger::{Claimant, Denied, Ledger, ReleaseError, Resource, Ticket};
pub use rate::{DocumentRates, ProducerRates, RateLimit};
