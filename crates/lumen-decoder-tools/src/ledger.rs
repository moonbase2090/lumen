//! Builds the ledger `lumen-evidence --decoder` judges.
//!
//! A campaign keeps, per format, two append-only JSON-lines files:
//! `NAME.samples.jsonl` with `{"at_cpu_hour": h, "edges": n}` and
//! `NAME.crashes.jsonl` with `{"at_cpu_hour": h, "kind": ".."}`. The CPU-hours
//! of a format are the last sample's `at_cpu_hour`, so a format with no
//! samples has none and cannot pass.

use serde_json::{Value, json};

/// Reads JSON lines, skipping blank ones.
///
/// # Errors
/// A line that is not JSON.
pub fn lines(text: &str) -> Result<Vec<Value>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("{e}: {l}")))
        .collect()
}

/// One format's entry from its samples and crashes.
///
/// # Errors
/// A sample without a numeric `at_cpu_hour`.
pub fn format_entry(name: &str, samples: &[Value], crashes: &[Value]) -> Result<Value, String> {
    let mut cpu_hours = 0.0f64;
    for s in samples {
        let h = s["at_cpu_hour"].as_f64().ok_or("a sample has no at_cpu_hour")?;
        cpu_hours = cpu_hours.max(h);
    }
    Ok(json!({
        "name": name,
        "cpu_hours": cpu_hours,
        "crashes": crashes,
        "coverage": samples,
    }))
}

/// The whole ledger.
#[must_use]
pub fn ledger(
    revision: &str,
    decoder_build: &str,
    image_path_build: &str,
    formats: Vec<Value>,
    escape_revision: &str,
    escape_passed: u32,
    escape_total: u32,
) -> Value {
    json!({
        "revision": revision,
        "decoder_build": decoder_build,
        "image_path_build": image_path_build,
        "formats": formats,
        "sandbox_escape": {
            "revision": escape_revision,
            "passed": escape_passed,
            "total": escape_total,
        },
    })
}
