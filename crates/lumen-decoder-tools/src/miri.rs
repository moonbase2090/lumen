//! The Miri record that `scripts/decoder-miri.sh` writes, and checks on it.

use serde_json::Value;

/// Checks that a record is well formed and that what it says is consistent:
/// every run has a known status; a run that passed has result lines and none of
/// them reports a failure; a run that failed carries the error that stopped it.
///
/// # Errors
/// A description of the first thing wrong.
pub fn validate(record: &Value) -> Result<(), String> {
    for key in [
        "toolchain",
        "miri",
        "host",
        "date",
        "repo_commit",
        "upstream_commit",
    ] {
        if record[key].as_str().is_none_or(str::is_empty) {
            return Err(format!("the record has no {key}"));
        }
    }
    let runs = record["runs"]
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or("the record has no runs")?;
    for run in runs {
        let name = run["name"].as_str().ok_or("a run has no name")?;
        let results = run["results"]
            .as_array()
            .ok_or(format!("{name}: no results list"))?;
        match run["status"].as_str() {
            Some("ok") => {
                if results.is_empty() {
                    return Err(format!("{name}: passed but has no test result line"));
                }
                for r in results {
                    let line = r.as_str().unwrap_or_default();
                    if !line.starts_with("test result: ok.") || !line.contains(" 0 failed;") {
                        return Err(format!("{name}: passed but a result line says: {line}"));
                    }
                }
            }
            Some("FAILED") => {
                if run["first_error"].as_str().is_none_or(str::is_empty) {
                    return Err(format!("{name}: failed without an error line"));
                }
            }
            other => return Err(format!("{name}: unknown status {other:?}")),
        }
    }
    Ok(())
}

/// Checks a fresh record against a committed one: both well formed, the same
/// upstream commit, and the same runs with the same status, result lines
/// (timings are already removed) and error. Toolchain, host, date and the
/// repository commit may differ.
///
/// # Errors
/// A description of the first difference.
pub fn same_results(fresh: &Value, committed: &Value) -> Result<(), String> {
    validate(fresh).map_err(|e| format!("fresh record: {e}"))?;
    validate(committed).map_err(|e| format!("committed record: {e}"))?;
    if fresh["upstream_commit"] != committed["upstream_commit"] {
        return Err("the upstream commit differs".into());
    }
    let (a, b) = (fresh["runs"].as_array(), committed["runs"].as_array());
    let (Some(a), Some(b)) = (a, b) else {
        return Err("no runs".into());
    };
    if a.len() != b.len() {
        return Err(format!("{} runs against {} committed", a.len(), b.len()));
    }
    for (x, y) in a.iter().zip(b) {
        for key in ["name", "status", "results", "first_error"] {
            if x[key] != y[key] {
                return Err(format!(
                    "run {}: {key} differs: {} against {}",
                    x["name"], x[key], y[key]
                ));
            }
        }
    }
    Ok(())
}
