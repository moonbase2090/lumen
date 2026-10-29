//! Render-log mode (criteria 2.6): the parts of the denial-of-service metric
//! that need a UI and a decoder.
//!
//! The driver has neither, so it cannot measure the longest UI stall, the
//! sustained frame rate or the memory use under a lying decoder itself. This
//! mode judges the log a terminal writes while the flood and a lying decoder
//! stream run against it, so the verdict does not depend on who read the
//! numbers. With no log, every part is `not measured`.
//!
//! # Render log format
//!
//! A JSON object:
//!
//! - `revision`: the log format revision, a string.
//! - `build`: the terminal build identity that was run, a string.
//! - `duration_ms`: the length of the run in milliseconds.
//! - `frames_ms`: the time of every frame drawn, in milliseconds from the
//!   start of the run, in order.
//! - `lying_decoder`: optional, `{"decoder_build": "..", "streams": n,
//!   "peak_rss_mib": n}`: `streams` is a whole number, the count of lying
//!   streams played (not a list), and `peak_rss_mib` is the peak resident
//!   memory of the terminal while they ran. A value that is not a whole number
//!   is refused.

use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use crate::HARNESS_VERSION;

/// The longest interval without a frame being drawn that 2.6 allows.
pub const STALL_LIMIT_MS: u64 = 2000;
/// Peak resident memory must stay under this many MiB.
pub const MEMORY_LIMIT_MIB: u64 = 512;

/// One part's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part<T> {
    /// The log did not carry this part.
    NotMeasured(String),
    /// Measured: the value and whether it is within the limit.
    Measured {
        /// The measured value.
        value: T,
        /// Within the limit.
        ok: bool,
    },
}

/// The result of judging a render log.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The terminal build identity, if a log was given.
    pub build: Option<String>,
    /// The longest interval without a frame, in milliseconds.
    pub stall_ms: Part<u64>,
    /// Frames per second over the run; reported, never gated.
    pub frame_rate: Option<f64>,
    /// Peak resident memory under the lying decoder, in MiB.
    pub memory_mib: Part<u64>,
}

impl Report {
    /// The exit status: 1 when a measured part is over its limit, else 3 when
    /// a part is not measured, else 0.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        let failed = matches!(self.stall_ms, Part::Measured { ok: false, .. })
            || matches!(self.memory_mib, Part::Measured { ok: false, .. });
        let missing = matches!(self.stall_ms, Part::NotMeasured(_))
            || matches!(self.memory_mib, Part::NotMeasured(_));
        if failed {
            1
        } else if missing {
            3
        } else {
            0
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
        if let Some(b) = &self.build {
            let _ = writeln!(out, "2.6 terminal_build={b}");
        }
        match &self.stall_ms {
            Part::NotMeasured(why) => {
                let _ = writeln!(out, "2.6 longest_ui_stall=not measured ({why})");
            }
            Part::Measured { value, ok } => {
                let _ = writeln!(
                    out,
                    "2.6 longest_ui_stall_ms={value} (limit {STALL_LIMIT_MS}) {}",
                    if *ok { "pass" } else { "FAIL" }
                );
            }
        }
        match self.frame_rate {
            Some(r) => {
                let _ = writeln!(
                    out,
                    "2.6 sustained_frame_rate_fps={r:.1} (reported, not gated)"
                );
            }
            None => {
                let _ = writeln!(out, "2.6 sustained_frame_rate=not measured (no frames)");
            }
        }
        match &self.memory_mib {
            Part::NotMeasured(why) => {
                let _ = writeln!(out, "2.6 lying_decoder_peak_rss=not measured ({why})");
            }
            Part::Measured { value, ok } => {
                let _ = writeln!(
                    out,
                    "2.6 lying_decoder_peak_rss_mib={value} (limit {MEMORY_LIMIT_MIB}) {}",
                    if *ok { "pass" } else { "FAIL" }
                );
            }
        }
        out
    }
}

fn not_measured(why: &str) -> Report {
    Report {
        build: None,
        stall_ms: Part::NotMeasured(why.to_owned()),
        frame_rate: None,
        memory_mib: Part::NotMeasured(why.to_owned()),
    }
}

/// Judge a render log given as JSON text.
///
/// # Errors
///
/// Returns a message for text that is not a render log.
pub fn judge(log: &str) -> Result<Report, String> {
    let v: Value = serde_json::from_str(log).map_err(|e| format!("render log: {e}"))?;
    let build = v["build"]
        .as_str()
        .filter(|b| !b.is_empty())
        .ok_or("render log: no build identity")?
        .to_owned();
    let duration = v["duration_ms"]
        .as_u64()
        .filter(|d| *d > 0)
        .ok_or("render log: duration_ms is missing or zero")?;
    let frames = v["frames_ms"]
        .as_array()
        .ok_or("render log: frames_ms is not a list")?
        .iter()
        .map(|f| {
            f.as_u64()
                .ok_or("render log: a frame time is not a whole number")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if frames.windows(2).any(|w| w[0] > w[1]) || frames.last().is_some_and(|l| *l > duration) {
        return Err("render log: frame times are out of order or past duration_ms".into());
    }
    let (stall_ms, frame_rate) = if frames.is_empty() {
        (Part::NotMeasured("no frames were drawn".to_owned()), None)
    } else {
        // The gaps include the wait for the first frame and the wait after the last.
        let edges = std::iter::once(0)
            .chain(frames.iter().copied())
            .chain(std::iter::once(duration))
            .collect::<Vec<_>>();
        let longest = edges.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
        #[allow(
            clippy::cast_precision_loss,
            reason = "frame counts and milliseconds are far below 2^52"
        )]
        let fps = frames.len() as f64 * 1000.0 / duration as f64;
        (
            Part::Measured {
                value: longest,
                ok: longest <= STALL_LIMIT_MS,
            },
            Some(fps),
        )
    };
    let memory_mib = match v.get("lying_decoder").filter(|d| !d.is_null()) {
        None => Part::NotMeasured("the log has no lying decoder run".to_owned()),
        Some(d) => {
            if d["decoder_build"].as_str().is_none_or(str::is_empty) {
                return Err("render log: lying_decoder has no decoder_build".into());
            }
            let streams = d["streams"]
                .as_u64()
                .ok_or("render log: lying_decoder.streams must be a whole number")?;
            if streams == 0 {
                Part::NotMeasured("no lying stream was played".to_owned())
            } else {
                let peak = d["peak_rss_mib"]
                    .as_u64()
                    .ok_or("render log: lying_decoder.peak_rss_mib is missing")?;
                Part::Measured {
                    value: peak,
                    ok: peak < MEMORY_LIMIT_MIB,
                }
            }
        }
    };
    Ok(Report {
        build: Some(build),
        stall_ms,
        frame_rate,
        memory_mib,
    })
}

/// Run render-log mode. With no `log` every part is `not measured`.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed log or a failed write.
pub fn run_render<W: Write>(
    log: Option<&Path>,
    commit: &str,
    out: W,
) -> Result<(Report, HeaderInfo), String> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: "render-log".to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let rec = Recorder::new(out, &header).map_err(|e| e.to_string())?;
    rec.finish().map_err(|e| e.to_string())?;
    let report = match log {
        None => not_measured("no render log was given and the driver has no UI or decoder"),
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

    fn good() -> Value {
        json!({
            "revision": "rl-1", "build": "t1", "duration_ms": 10000,
            "frames_ms": [100, 1100, 2100, 3100, 4100, 5100, 6100, 7100, 8100, 9100],
            "lying_decoder": {"decoder_build": "d1", "streams": 4, "peak_rss_mib": 300}
        })
    }

    fn judged(v: &Value) -> Report {
        match judge(&v.to_string()) {
            Ok(r) => r,
            Err(e) => unreachable!("log judges: {e}"),
        }
    }

    #[test]
    fn a_log_within_every_limit_exits_0() {
        let r = judged(&good());
        assert_eq!(r.exit_code(), 0);
        assert_eq!(
            r.stall_ms,
            Part::Measured {
                value: 1000,
                ok: true
            }
        );
        assert_eq!(r.frame_rate, Some(1.0));
    }

    #[test]
    fn the_gap_before_the_first_and_after_the_last_frame_count() {
        let mut v = good();
        v["frames_ms"] = json!([2500, 3000]);
        v["duration_ms"] = json!(3100);
        assert_eq!(
            judged(&v).stall_ms,
            Part::Measured {
                value: 2500,
                ok: false
            }
        );
        v["frames_ms"] = json!([100, 600]);
        v["duration_ms"] = json!(3000);
        assert_eq!(
            judged(&v).stall_ms,
            Part::Measured {
                value: 2400,
                ok: false
            }
        );
    }

    #[test]
    fn a_stall_of_exactly_2_seconds_is_allowed_and_one_millisecond_more_is_not() {
        let mut v = good();
        v["frames_ms"] = json!([0, 2000]);
        v["duration_ms"] = json!(2000);
        assert_eq!(judged(&v).exit_code(), 0);
        v["frames_ms"] = json!([0, 2001]);
        v["duration_ms"] = json!(2001);
        assert_eq!(judged(&v).exit_code(), 1);
    }

    #[test]
    fn peak_memory_must_stay_under_512_mib() {
        let mut v = good();
        v["lying_decoder"]["peak_rss_mib"] = json!(511);
        assert_eq!(judged(&v).exit_code(), 0);
        v["lying_decoder"]["peak_rss_mib"] = json!(512);
        assert_eq!(judged(&v).exit_code(), 1);
    }

    #[test]
    fn the_frame_rate_is_reported_and_never_fails_the_run() {
        let mut v = good();
        v["frames_ms"] = json!([1000, 2000, 3000]);
        v["duration_ms"] = json!(3500);
        let r = judged(&v);
        assert_eq!(r.exit_code(), 0);
        assert!(r.frame_rate.is_some_and(|f| f < 1.0));
    }

    #[test]
    fn a_missing_part_is_not_measured_and_exits_3() {
        let mut v = good();
        v["lying_decoder"] = Value::Null;
        assert_eq!(judged(&v).exit_code(), 3);
        let mut v = good();
        v["lying_decoder"]["streams"] = json!(0);
        assert_eq!(judged(&v).exit_code(), 3);
        let mut v = good();
        v["frames_ms"] = json!([]);
        assert_eq!(judged(&v).exit_code(), 3);
    }

    #[test]
    fn a_failure_wins_over_a_missing_part() {
        let mut v = good();
        v["frames_ms"] = json!([0, 5000]);
        v["lying_decoder"] = Value::Null;
        assert_eq!(judged(&v).exit_code(), 1);
    }

    #[test]
    fn bad_logs_are_refused() {
        for bad in [
            json!({"build": "", "duration_ms": 1, "frames_ms": []}),
            json!({"build": "t", "duration_ms": 0, "frames_ms": []}),
            json!({"build": "t", "duration_ms": 10, "frames_ms": [5, 3]}),
            json!({"build": "t", "duration_ms": 10, "frames_ms": [11]}),
        ] {
            assert!(judge(&bad.to_string()).is_err());
        }
        let mut v = good();
        v["lying_decoder"]["decoder_build"] = json!("");
        assert!(judge(&v.to_string()).is_err());
        for bad_streams in [
            json!(["a", "b"]),
            json!("4"),
            json!(1.5),
            json!(-1),
            Value::Null,
        ] {
            let mut v = good();
            v["lying_decoder"]["streams"] = bad_streams;
            assert!(judge(&v.to_string()).is_err());
        }
        let mut v = good();
        v["lying_decoder"]
            .as_object_mut()
            .map(|o| o.remove("streams"));
        assert!(judge(&v.to_string()).is_err());
    }

    #[test]
    fn no_log_is_not_measured() {
        let Ok((report, header)) = run_render(None, "sha", Vec::new()) else {
            unreachable!("no log runs")
        };
        assert_eq!(report.exit_code(), 3);
        assert!(
            report
                .summary(&header)
                .contains("longest_ui_stall=not measured")
        );
    }
}
