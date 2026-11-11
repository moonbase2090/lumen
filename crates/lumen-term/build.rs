//! Hands the build the gate records it ships with (`release/gate-records.txt`)
//! and the two build identities from the record of the platform being built.
//! A tree without the file, or a platform without a line in it, gets no
//! records, and its images stay locked. The build does not decide whether a
//! record is true: a test recomputes the identities from the sources and
//! compares them with the record, and the release check does too.

use std::path::Path;

fn main() {
    let records = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release/gate-records.txt");
    println!("cargo:rerun-if-changed={}", records.display());
    println!("cargo:rerun-if-changed=build.rs");
    let Ok(text) = std::fs::read_to_string(&records) else {
        return;
    };
    let target = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => "macos",
        Ok("windows") => "windows",
        _ => "linux",
    };
    let mine = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|f| f.len() == 5 && f[0] == target);
    let Some(f) = mine else {
        return;
    };
    println!("cargo:rustc-env=LUMEN_DECODER_BUILD={}", f[1]);
    println!("cargo:rustc-env=LUMEN_IMAGE_PATH_BUILD={}", f[2]);
    // One record per line; a newline cannot go in a cargo directive, so the
    // lines are joined with `;` and split again by the terminal.
    let joined = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    println!("cargo:rustc-env=LUMEN_GATE_RECORDS={joined}");
}
