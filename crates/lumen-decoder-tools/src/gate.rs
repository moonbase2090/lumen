//! A format's state against the decoder gate, from the samples and crashes a
//! campaign keeps. The rules are the judge's in `lumen-evidence`
//! (`decoder::format_of`): the tools test checks the two agree.

use serde_json::Value;

/// The floor: CPU-hours of fuzzing a format needs before it is judged.
pub const MIN_CPU_HOURS: f64 = 250.0;
/// The stop rule's window: no new edge in this many CPU-hours means the run
/// may stop.
pub const WINDOW_CPU_HOURS: f64 = 168.0;
/// The cap: a run does not go past this many CPU-hours for one format.
pub const MAX_CPU_HOURS: f64 = 1200.0;

/// Where a format stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// CPU-hours so far: the last sample's hour.
    pub cpu_hours: f64,
    /// The most coverage edges seen.
    pub edges: f64,
    /// Crashes recorded.
    pub crashes: usize,
    /// The run went past the cap.
    pub over_cap: bool,
    /// CPU-hours since the latest crash (all of them if there was none).
    pub hours_since_new_crash: f64,
    /// CPU-hours since coverage last grew.
    pub hours_since_coverage_gain: f64,
    /// No coverage gain over the last window, with a reading from before it.
    pub plateau: bool,
    /// Every part of the gate holds for this format. Any crash stops it from
    /// holding, however long ago the crash was found.
    pub holds: bool,
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("{what} is not a number"))
}

/// Computes the status.
///
/// # Errors
/// A sample or crash without a numeric hour, or a sample without edges.
pub fn status(samples: &[Value], crashes: &[Value]) -> Result<Status, String> {
    status_with(samples, crashes, MIN_CPU_HOURS, WINDOW_CPU_HOURS)
}

/// [`status`] with other thresholds. Only the test of the campaign driver uses
/// it, to reach "the gate holds" within seconds; the gate's own numbers are the
/// constants above.
///
/// # Errors
/// As [`status`].
pub fn status_with(
    samples: &[Value],
    crashes: &[Value],
    min_hours: f64,
    window: f64,
) -> Result<Status, String> {
    let mut cpu_hours = 0.0_f64;
    for s in samples {
        cpu_hours = cpu_hours.max(num(&s["at_cpu_hour"], "a sample's at_cpu_hour")?);
    }
    let from = cpu_hours - window;
    let mut last_crash = None::<f64>;
    for c in crashes {
        let at = num(&c["at_cpu_hour"], "a crash's at_cpu_hour")?;
        last_crash = Some(last_crash.map_or(at, |l| l.max(at)));
    }
    let (mut before, mut overall) = (None::<f64>, 0.0_f64);
    let mut ordered: Vec<(f64, f64)> = Vec::new();
    for p in samples {
        let at = num(&p["at_cpu_hour"], "a sample's at_cpu_hour")?;
        let edges = num(&p["edges"], "a sample's edges")?;
        overall = overall.max(edges);
        if at <= from {
            before = Some(before.map_or(edges, |b| b.max(edges)));
        }
        ordered.push((at, edges));
    }
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The hour at which the running maximum last rose.
    let (mut best, mut gain_at) = (f64::MIN, 0.0_f64);
    for (at, edges) in ordered {
        if edges > best {
            best = edges;
            gain_at = at;
        }
    }
    let plateau = before.is_some_and(|b| b >= overall);
    let over_cap = cpu_hours > MAX_CPU_HOURS;
    let holds = cpu_hours >= min_hours && crashes.is_empty() && plateau && !over_cap;
    Ok(Status {
        cpu_hours,
        edges: overall,
        crashes: crashes.len(),
        over_cap,
        hours_since_new_crash: cpu_hours - last_crash.unwrap_or(0.0),
        hours_since_coverage_gain: cpu_hours - gain_at,
        plateau,
        holds,
    })
}
