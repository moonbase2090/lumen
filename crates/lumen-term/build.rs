//! Hands the build the gate records it ships with (`release/gate-records.txt`)
//! and the two build identities from the record of the platform being built.
//! The three `LUMEN_*` values are always set, to locked values when there is
//! no record, so nothing in the environment of the build can unlock images.
//! The build does not decide whether a record is true: a test recomputes the
//! identities from the sources and compares them with the record, and the
//! release check does too.

#[path = "build_support/gate.rs"]
mod gate;

use std::path::Path;

fn main() {
    let records = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release/gate-records.txt");
    println!("cargo:rerun-if-changed={}", records.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support/gate.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    let text = std::fs::read_to_string(&records).ok();
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    for line in gate::directives(text.as_deref(), &target) {
        println!("{line}");
    }
}
