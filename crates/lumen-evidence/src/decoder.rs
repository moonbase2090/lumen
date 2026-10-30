//! Decoder robustness mode (criteria 2.5): judges a fuzzing ledger.
//!
//! The driver does not fuzz. There is no image decoder in this repository and
//! images are off, so there is nothing to fuzz yet. This mode reads the
//! ledger that a decoder fuzzing run writes and applies the release criteria's
//! rule (a floor of 250 CPU-hours, a stop after 168 CPU-hours with no new crash
//! or coverage edge, a cap of 1200, and any crash fails the build identity) to
//! it, so a run's verdict does not depend on who read the numbers. With no
//! ledger it reports `not measured`.
//!
//! # Ledger format
//!
//! A JSON object:
//!
//! - `revision`: the ledger format revision, a string.
//! - `decoder_build` and `image_path_build`: the build identities the run
//!   fuzzed; the gate covers both.
//! - `formats`: one entry per enabled image format with `name`, `cpu_hours`,
//!   `crashes` (each `{"at_cpu_hour": h, "kind": ".."}`, where a hang over
//!   2 s, memory over 512 MiB and a sanitizer report all count as crashes)
//!   and `coverage` (each `{"at_cpu_hour": h, "edges": n}`, the cumulative
//!   coverage edges found by then).
//! - `sandbox_escape`: `{"revision": "..", "passed": n, "total": n}` for the
//!   named test set.

use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use crate::HARNESS_VERSION;

/// The floor: CPU-hours of fuzzing each enabled format needs before it is judged.
pub const MIN_CPU_HOURS: f64 = 250.0;
/// The stop rule's window: the run stops when it has seen no new coverage edge
/// (and no crash) in this many CPU-hours.
pub const WINDOW_CPU_HOURS: f64 = 168.0;
/// The cap: a run does not go past this many CPU-hours for one format.
pub const MAX_CPU_HOURS: f64 = 1200.0;

/// One format's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatResult {
    /// The format's name.
    pub name: String,
    /// CPU-hours of fuzzing.
    pub cpu_hours: f64,
    /// Crashes found. Any crash fails the build identity the ledger covers; a
    /// crash does not age out of the window.
    pub crashes: usize,
    /// The run went past [`MAX_CPU_HOURS`].
    pub over_cap: bool,
    /// No new coverage edge in the last window.
    pub plateau: bool,
    /// Every part of the threshold holds for this format.
    pub pass: bool,
}

/// The decoder robustness result.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// No ledger was given, or it names no enabled format.
    NotMeasured(String),
    /// The ledger was judged.
    Judged {
        /// The decoder build identity.
        decoder_build: String,
        /// The terminal image-path build identity.
        image_path_build: String,
        /// One result per format.
        formats: Vec<FormatResult>,
        /// The sandbox escape set: revision, passed, total.
        sandbox: (String, u64, u64),
    },
}

impl Report {
    /// True when the run measured the gate and every part of it holds.
    #[must_use]
    pub fn passes(&self) -> bool {
        match self {
            Report::NotMeasured(_) => false,
            Report::Judged {
                formats, sandbox, ..
            } => formats.iter().all(|f| f.pass) && sandbox.2 > 0 && sandbox.1 == sandbox.2,
        }
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit, header.harness, header.corpus, header.session_matrix, header.sinks
        );
        match self {
            Report::NotMeasured(why) => {
                let _ = writeln!(out, "2.5=not measured ({why}); images stay off");
            }
            Report::Judged {
                decoder_build,
                image_path_build,
                formats,
                sandbox,
            } => {
                let _ = writeln!(
                    out,
                    "2.5 decoder_build={decoder_build} image_path_build={image_path_build}"
                );
                for f in formats {
                    let _ = writeln!(
                        out,
                        "format {} cpu_hours={} (floor {MIN_CPU_HOURS}, cap {MAX_CPU_HOURS}) crashes={} over_cap={} plateau={} {}",
                        f.name,
                        f.cpu_hours,
                        f.crashes,
                        f.over_cap,
                        f.plateau,
                        if f.pass { "pass" } else { "FAIL" }
                    );
                }
                let _ = writeln!(
                    out,
                    "sandbox_escape revision={} passed={}/{}",
                    sandbox.0, sandbox.1, sandbox.2
                );
                let _ = writeln!(
                    out,
                    "2.5 decoder gate: {}",
                    if self.passes() { "pass" } else { "FAIL" }
                );
            }
        }
        out
    }
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("{what} is not a number"))
}

fn text(v: &Value, what: &str) -> Result<String, String> {
    match v.as_str() {
        Some(s) if !s.is_empty() => Ok(s.to_owned()),
        _ => Err(format!("{what} is missing")),
    }
}

fn format_of(v: &Value) -> Result<FormatResult, String> {
    let name = text(&v["name"], "a format name")?;
    let ctx = |e: String| format!("format {name}: {e}");
    let cpu_hours = num(&v["cpu_hours"], "cpu_hours").map_err(ctx)?;
    let from = cpu_hours - WINDOW_CPU_HOURS;
    let crashes = v["crashes"]
        .as_array()
        .ok_or_else(|| ctx("crashes is not a list".into()))?;
    for c in crashes {
        num(&c["at_cpu_hour"], "a crash's at_cpu_hour").map_err(ctx)?;
    }
    let coverage = v["coverage"]
        .as_array()
        .ok_or_else(|| ctx("coverage is not a list".into()))?;
    let (mut before, mut overall) = (None::<f64>, 0.0_f64);
    for p in coverage {
        let at = num(&p["at_cpu_hour"], "a coverage at_cpu_hour").map_err(ctx)?;
        let edges = num(&p["edges"], "coverage edges").map_err(ctx)?;
        overall = overall.max(edges);
        if at <= from {
            before = Some(before.map_or(edges, |b| b.max(edges)));
        }
    }
    // The plateau needs a coverage reading from before the window to compare to.
    let plateau = before.is_some_and(|b| b >= overall);
    let over_cap = cpu_hours > MAX_CPU_HOURS;
    // A crash ends the run for this build identity, however long ago it was
    // found; reaching the cap before the stop rule holds is not a pass either,
    // because the plateau test above is the stop rule.
    let pass = cpu_hours >= MIN_CPU_HOURS && crashes.is_empty() && plateau && !over_cap;
    Ok(FormatResult {
        name,
        cpu_hours,
        crashes: crashes.len(),
        over_cap,
        plateau,
        pass,
    })
}

/// Judge a ledger given as JSON text.
///
/// # Errors
///
/// Returns a message for text that is not a ledger.
pub fn judge(ledger: &str) -> Result<Report, String> {
    let v: Value = serde_json::from_str(ledger).map_err(|e| format!("ledger: {e}"))?;
    let formats = v["formats"].as_array().ok_or("ledger: no formats list")?;
    if formats.is_empty() {
        return Ok(Report::NotMeasured(
            "the ledger enables no image format".into(),
        ));
    }
    let formats = formats
        .iter()
        .map(format_of)
        .collect::<Result<Vec<_>, _>>()?;
    let s = &v["sandbox_escape"];
    let sandbox = (
        text(&s["revision"], "the sandbox escape test set revision")?,
        s["passed"]
            .as_u64()
            .ok_or("sandbox_escape.passed is missing")?,
        s["total"]
            .as_u64()
            .ok_or("sandbox_escape.total is missing")?,
    );
    Ok(Report::Judged {
        decoder_build: text(&v["decoder_build"], "decoder_build")?,
        image_path_build: text(&v["image_path_build"], "image_path_build")?,
        formats,
        sandbox,
    })
}

/// Run decoder mode. With no `ledger` the result is `not measured`.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed ledger or a failed write.
pub fn run_decoder<W: Write>(
    ledger: Option<&Path>,
    commit: &str,
    log: W,
) -> Result<(Report, HeaderInfo), String> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: "decoder".to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let rec = Recorder::new(log, &header).map_err(|e| e.to_string())?;
    rec.finish().map_err(|e| e.to_string())?;
    let report = match ledger {
        None => Report::NotMeasured(
            "no decoder fuzzing ledger was given and no image decoder exists in this build".into(),
        ),
        Some(p) => {
            judge(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?)?
        }
    };
    Ok((report, header))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A ledger for one format that meets every part of the threshold.
    fn good() -> Value {
        json!({
            "revision": "fz-1",
            "decoder_build": "d1",
            "image_path_build": "p1",
            "formats": [{
                "name": "png",
                "cpu_hours": 1200.0,
                "crashes": [],
                "coverage": [
                    {"at_cpu_hour": 100.0, "edges": 900},
                    {"at_cpu_hour": 1000.0, "edges": 950},
                    {"at_cpu_hour": 1200.0, "edges": 950}
                ]
            }],
            "sandbox_escape": {"revision": "se-1", "passed": 12, "total": 12}
        })
    }

    fn judged(v: &Value) -> Report {
        match judge(&v.to_string()) {
            Ok(r) => r,
            Err(e) => unreachable!("ledger judges: {e}"),
        }
    }

    #[test]
    fn a_ledger_that_meets_the_threshold_passes() {
        assert!(judged(&good()).passes());
    }

    #[test]
    fn too_few_cpu_hours_fails_and_the_floor_itself_is_enough() {
        let mut v = good();
        v["formats"][0]["crashes"] = json!([]);
        v["formats"][0]["cpu_hours"] = json!(249.0);
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 80.0, "edges": 950}]);
        assert!(!judged(&v).passes());
        v["formats"][0]["cpu_hours"] = json!(250.0);
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 82.0, "edges": 950}]);
        assert!(judged(&v).passes());
    }

    #[test]
    fn the_earlier_1000_hour_floor_no_longer_applies() {
        let mut v = good();
        v["formats"][0]["crashes"] = json!([]);
        v["formats"][0]["cpu_hours"] = json!(400.0);
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 100.0, "edges": 950}]);
        assert!(judged(&v).passes());
    }

    #[test]
    fn any_crash_fails_however_long_ago_it_was_found() {
        let mut v = good();
        for at in [1.0, 40.0, 1031.0, 1033.0, 1200.0] {
            v["formats"][0]["crashes"] = json!([{"at_cpu_hour": at, "kind": "sanitizer"}]);
            assert!(!judged(&v).passes(), "a crash at {at} must fail");
        }
        v["formats"][0]["crashes"] = json!([]);
        assert!(judged(&v).passes());
    }

    #[test]
    fn a_run_past_the_cap_fails_and_the_cap_itself_does_not() {
        let mut v = good();
        v["formats"][0]["crashes"] = json!([]);
        v["formats"][0]["cpu_hours"] = json!(1200.0);
        assert!(judged(&v).passes());
        v["formats"][0]["cpu_hours"] = json!(1200.5);
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 100.0, "edges": 950}]);
        assert!(!judged(&v).passes());
    }

    #[test]
    fn a_run_that_reaches_the_cap_with_coverage_still_growing_has_not_passed() {
        let mut v = good();
        v["formats"][0]["crashes"] = json!([]);
        v["formats"][0]["coverage"] = json!([
            {"at_cpu_hour": 100.0, "edges": 900},
            {"at_cpu_hour": 1100.0, "edges": 951}
        ]);
        assert!(!judged(&v).passes());
    }

    #[test]
    fn the_window_edge_belongs_to_the_older_side() {
        let mut v = good();
        v["formats"][0]["crashes"] = json!([]);
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 1032.0, "edges": 950}]);
        assert!(judged(&v).passes());
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 1033.0, "edges": 950}]);
        assert!(
            !judged(&v).passes(),
            "no reading at or before the window start"
        );
    }

    #[test]
    fn new_coverage_inside_the_window_means_no_plateau() {
        let mut v = good();
        v["formats"][0]["coverage"] = json!([
            {"at_cpu_hour": 100.0, "edges": 900},
            {"at_cpu_hour": 1200.0, "edges": 951}
        ]);
        assert!(!judged(&v).passes());
    }

    #[test]
    fn a_plateau_needs_a_reading_from_before_the_window() {
        let mut v = good();
        v["formats"][0]["coverage"] = json!([{"at_cpu_hour": 1100.0, "edges": 950}]);
        assert!(!judged(&v).passes());
    }

    #[test]
    fn every_format_must_pass() {
        let mut v = good();
        let mut second = v["formats"][0].clone();
        second["name"] = json!("webp");
        second["cpu_hours"] = json!(10.0);
        v["formats"] = json!([v["formats"][0].clone(), second]);
        assert!(!judged(&v).passes());
    }

    #[test]
    fn the_sandbox_set_must_pass_in_full_and_not_be_empty() {
        let mut v = good();
        v["sandbox_escape"]["passed"] = json!(11);
        assert!(!judged(&v).passes());
        v["sandbox_escape"] = json!({"revision": "se-1", "passed": 0, "total": 0});
        assert!(!judged(&v).passes());
    }

    #[test]
    fn a_ledger_without_a_build_identity_or_set_revision_is_refused() {
        let mut v = good();
        v["image_path_build"] = json!("");
        assert!(judge(&v.to_string()).is_err());
        let mut v = good();
        v["sandbox_escape"]["revision"] = json!(null);
        assert!(judge(&v.to_string()).is_err());
    }

    #[test]
    fn no_enabled_format_and_no_ledger_are_not_measured() {
        let mut v = good();
        v["formats"] = json!([]);
        assert!(matches!(judged(&v), Report::NotMeasured(_)));
        let Ok((report, header)) = run_decoder(None, "sha", Vec::new()) else {
            unreachable!("no ledger runs")
        };
        assert!(matches!(report, Report::NotMeasured(_)));
        assert!(report.summary(&header).contains("2.5=not measured"));
        assert!(!report.passes());
    }
}
