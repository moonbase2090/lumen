//! The sandbox escape result, read from a `cargo test` run of the G4 suite.

/// The passed and total counts of the test binary whose name contains `binary`.
///
/// `cargo test` prints `Running .../deps/NAME-HASH` before each binary's
/// `test result:` line; the line after the matching `Running` line is used.
#[must_use]
pub fn counts(output: &str, binary: &str) -> Option<(u32, u32)> {
    let mut in_binary = false;
    for line in output.lines() {
        if line.contains("Running ") {
            in_binary = line.contains(binary);
        } else if in_binary && line.starts_with("test result:") {
            let num = |after: &str| -> Option<u32> {
                let rest = line.split(after).next()?;
                rest.split_whitespace().last()?.parse().ok()
            };
            let passed = num(" passed")?;
            let failed = num(" failed")?;
            let ignored = num(" ignored")?;
            return Some((passed, passed + failed + ignored));
        }
    }
    None
}
