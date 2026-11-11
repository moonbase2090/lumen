//! What the terminal's build script tells the compiler about the gate records.
//! A separate file so a test can run the same code (`tests/build_gate.rs`).

/// The value that never unlocks, for an identity.
pub const LOCKED: &str = "absent";

/// The records that apply to `target_os`, as the build's rustc environment:
/// the two identities from the record for that platform, and the records.
/// Every call sets all three variables, so a value inherited from the
/// environment of the build can never stand in for a missing record. A target
/// that is not Linux, macOS or Windows, a missing file and a file with no
/// record for the target all give the locked values.
#[must_use]
pub fn directives(records: Option<&str>, target_os: &str) -> Vec<String> {
    let locked = || {
        vec![
            format!("cargo:rustc-env=LUMEN_DECODER_BUILD={LOCKED}"),
            format!("cargo:rustc-env=LUMEN_IMAGE_PATH_BUILD={LOCKED}"),
            "cargo:rustc-env=LUMEN_GATE_RECORDS=".to_owned(),
        ]
    };
    let Some(text) = records else {
        return locked();
    };
    if !matches!(target_os, "linux" | "macos" | "windows") {
        return locked();
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mine = lines
        .iter()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|f| f.len() == 5 && f[0] == target_os);
    let Some(f) = mine else {
        return locked();
    };
    vec![
        format!("cargo:rustc-env=LUMEN_DECODER_BUILD={}", f[1]),
        format!("cargo:rustc-env=LUMEN_IMAGE_PATH_BUILD={}", f[2]),
        // A newline cannot go in a cargo directive: the lines are joined with
        // `;` and split again by the terminal.
        format!("cargo:rustc-env=LUMEN_GATE_RECORDS={}", lines.join(";")),
    ]
}
