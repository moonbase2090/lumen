//! The notification guard now lives in `lumen-term`, where the terminal uses
//! it on the real path; the harness model uses the same code.

pub use lumen_term::guard::*;
