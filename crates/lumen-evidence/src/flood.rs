//! Flood mode (criteria 2.6): an unauthenticated producer writes Track 0 into
//! one command's output as fast as the layer will take it.
//!
//! The run measures what the byte pipeline can: the blocks and rich bytes it
//! lets through against the 25% share of the budget, the notices it gives, the
//! resident memory of this process, and the longest single `feed` call. It has
//! no UI, so a UI stall, the sustained frame rate and the lying-decoder stream
//! are reported as not measured. The longest `feed` call is a proxy for how
//! long a caller of the pipeline would be held up; it is not a UI stall.
//!
//! The producer rotates through four kinds of read, each [`READ_BYTES`] long:
//! valid paragraph frames, headers with a wrong tag, bare introducers, and
//! ordinary output.

use crate::{AnyLayer, Arm, FULL_BYTES, FULL_FRAMES, HARNESS_VERSION};
use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::time::{Duration, Instant};

/// Bytes in one producer read.
pub const READ_BYTES: usize = 64 * 1024;

/// Peak resident memory the decoder limit allows, in MiB (criteria 2.6).
pub const MEMORY_LIMIT_MIB: u64 = 512;

const TAG: [u8; 8] = [0x8a, b'L', b'U', b'M', b'E', b'N', 0x01, 0xf3];

/// A valid unsealed paragraph frame.
fn paragraph() -> Vec<u8> {
    let text = [b'x'; 100];
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(0x80);
    v.extend_from_slice(&102u32.to_be_bytes());
    v.extend_from_slice(&100u16.to_be_bytes());
    v.extend_from_slice(&text);
    v
}

/// A header whose tag is wrong, with a short body.
fn wrong_tag() -> Vec<u8> {
    let mut v = b"\x1b_BADTAG00".to_vec();
    v.push(0x80);
    v.extend_from_slice(&4u32.to_be_bytes());
    v.extend_from_slice(&[0, 2, b'h', b'i']);
    v
}

/// The four read kinds, each filled to [`READ_BYTES`].
fn reads() -> [Vec<u8>; 4] {
    let fill = |unit: &[u8]| unit.repeat(READ_BYTES / unit.len() + 1)[..READ_BYTES].to_vec();
    [
        fill(&paragraph()),
        fill(&wrong_tag()),
        fill(b"\x1b_"),
        fill(b"ordinary output line\n"),
    ]
}

/// Resident memory of this process in KiB, and where the number came from.
/// On Linux it is the kernel's high-water mark; elsewhere it is the current
/// resident size, so the run takes the maximum over its samples.
fn resident_kib() -> Option<(u64, &'static str)> {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let kib = status
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok());
        return kib.map(|k| (k, "VmHWM"));
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kib = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some((kib, "ps sampled maximum"))
}

/// The result of a flood run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloodReport {
    /// The arm replayed.
    pub arm: &'static str,
    /// Wall-clock length of the run in milliseconds.
    pub elapsed_ms: u64,
    /// Bytes written by the producer.
    pub bytes_in: u64,
    /// Reads written by the producer.
    pub reads: u64,
    /// Items by kind.
    pub kinds: BTreeMap<&'static str, u64>,
    /// Payload bytes of the items by kind.
    pub lens: BTreeMap<&'static str, u64>,
    /// The layer's counters after the stream ended.
    pub counters: String,
    /// The most blocks the layer may draw for one command.
    pub max_blocks: u64,
    /// The most block bytes the layer may draw for one command.
    pub max_block_bytes: u64,
    /// Longest single `feed` call in microseconds.
    pub longest_feed_us: u64,
    /// Peak resident memory in KiB, with its source, if it could be read.
    pub peak_rss: Option<(u64, &'static str)>,
    /// The time, in milliseconds from the start, at which a call into the layer
    /// returned, kept when at least [`RETURN_RESOLUTION_MS`] after the previous
    /// kept one. A gap read from these times is never shorter than the real
    /// gap, and is at most [`RETURN_RESOLUTION_MS`] longer.
    pub returns_ms: Vec<u64>,
}

impl FloodReport {
    /// The render log of this run (criteria 5.8) on the `pipeline` basis.
    ///
    /// The driver has no UI, so a "frame" here is a return from the layer, not
    /// a drawn frame. The log says so in `basis`, carries no `lying_decoder`
    /// (there is no decoder) and the judge reports its stall as a proxy.
    #[must_use]
    pub fn render_log(&self, build: &str) -> String {
        let frames: Vec<String> = self.returns_ms.iter().map(u64::to_string).collect();
        format!(
            "{{\"revision\":\"rl-1\",\"basis\":\"pipeline\",\"build\":\"{build}\",\"duration_ms\":{},\"frames_ms\":[{}]}}\n",
            self.elapsed_ms
                .max(self.returns_ms.last().copied().unwrap_or(0))
                .max(1),
            frames.join(",")
        )
    }
}

/// The smallest spacing, in milliseconds, between two return times kept for
/// the render log. It keeps a 600-second run to tens of thousands of entries.
pub const RETURN_RESOLUTION_MS: u64 = 10;

/// Blocks (or their bytes) the layer drew, in the container or the strip.
fn drawn(table: &BTreeMap<&'static str, u64>) -> u64 {
    ["block", "background"]
        .iter()
        .map(|k| table.get(k).copied().unwrap_or(0))
        .sum()
}

impl FloodReport {
    /// The unauthenticated producer drew no more than its share.
    #[must_use]
    pub fn within_budget(&self) -> bool {
        drawn(&self.kinds) <= self.max_blocks
            && drawn(&self.lens) <= self.max_block_bytes
            && self.kinds.get("degraded").copied().unwrap_or(0) <= 1
    }

    /// Peak memory is under the decoder limit, or could not be read.
    #[must_use]
    pub fn memory_ok(&self) -> Option<bool> {
        self.peak_rss.map(|(kib, _)| kib / 1024 < MEMORY_LIMIT_MIB)
    }

    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} features={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit,
            header.features,
            header.harness,
            header.corpus,
            header.session_matrix,
            header.sinks
        );
        let _ = writeln!(
            out,
            "flood elapsed_ms={} reads={} bytes_in={}",
            self.elapsed_ms, self.reads, self.bytes_in
        );
        let _ = writeln!(out, "items={:?} bytes={:?}", self.kinds, self.lens);
        let _ = writeln!(
            out,
            "2.6 budget={} drawn={} (limit {}) drawn_bytes={} (limit {}) degraded_notices={}",
            if self.within_budget() {
                "within"
            } else {
                "VIOLATION"
            },
            drawn(&self.kinds),
            self.max_blocks,
            drawn(&self.lens),
            self.max_block_bytes,
            self.kinds.get("degraded").copied().unwrap_or(0)
        );
        match self.peak_rss {
            Some((kib, src)) => {
                let _ = writeln!(
                    out,
                    "2.6 peak_rss_mib={} ({src}; limit {MEMORY_LIMIT_MIB}) {}",
                    kib / 1024,
                    if self.memory_ok() == Some(true) {
                        "within"
                    } else {
                        "OVER"
                    }
                );
            }
            None => {
                let _ = writeln!(out, "2.6 peak_rss=not measured (could not be read)");
            }
        }
        let _ = writeln!(
            out,
            "2.6 longest_feed_ms={}.{:03} (proxy; UI stall not measured, no UI)",
            self.longest_feed_us / 1000,
            self.longest_feed_us % 1000
        );
        let _ = writeln!(out, "2.6 frame_rate=not measured (no UI)");
        let _ = writeln!(out, "2.6 lying_decoder=not measured (no decoder)");
        let _ = writeln!(out, "counters={}", self.counters);
        out
    }
}

/// Flood `arm` for `duration` and write one log line per second to `log`.
///
/// # Errors
///
/// Returns the write error, if any.
pub fn run_flood<W: Write>(
    arm: Arm,
    commit: &str,
    duration: Duration,
    log: W,
) -> std::io::Result<(FloodReport, HeaderInfo)> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: arm.features().to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let mut rec = Recorder::new(log, &header)?;
    rec.case("flood")?;
    let mut layer = AnyLayer::new(arm);
    layer.open_container();
    let inputs = reads();
    let (mut r, mut window) = (0u64, BTreeMap::<&'static str, usize>::new());
    let mut report = FloodReport {
        arm: arm.features(),
        elapsed_ms: 0,
        bytes_in: 0,
        reads: 0,
        kinds: BTreeMap::new(),
        lens: BTreeMap::new(),
        counters: String::new(),
        max_blocks: (FULL_FRAMES / 4) as u64,
        max_block_bytes: (FULL_BYTES / 4) as u64,
        longest_feed_us: 0,
        peak_rss: None,
        returns_ms: Vec::new(),
    };
    let (start, mut last_flush, mut last_rss) = (Instant::now(), Instant::now(), Instant::now());
    let mut window_in = 0usize;
    let take =
        |items: Vec<(&'static str, usize)>, report: &mut FloodReport, w: &mut BTreeMap<_, _>| {
            for (kind, len) in items {
                *report.kinds.entry(kind).or_insert(0) += 1;
                *report.lens.entry(kind).or_insert(0) += len as u64;
                *w.entry(kind).or_insert(0) += len;
            }
        };
    while start.elapsed() < duration {
        let bytes = &inputs[(r % 4) as usize];
        let now_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let t = Instant::now();
        let items = layer.feed_raw(bytes, false, false, now_ms);
        report.longest_feed_us = report
            .longest_feed_us
            .max(u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX));
        let returned = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        if report
            .returns_ms
            .last()
            .is_none_or(|last| returned >= last + RETURN_RESOLUTION_MS)
        {
            report.returns_ms.push(returned);
        }
        take(items, &mut report, &mut window);
        r += 1;
        window_in += bytes.len();
        report.bytes_in += bytes.len() as u64;
        if last_rss.elapsed() >= Duration::from_millis(100) {
            last_rss = Instant::now();
            if let Some((kib, src)) = resident_kib() {
                let best = report.peak_rss.map_or(0, |(k, _)| k);
                report.peak_rss = Some((kib.max(best), src));
            }
        }
        if last_flush.elapsed() >= Duration::from_secs(1) {
            last_flush = Instant::now();
            flush(&mut rec, start, window_in, &mut window)?;
            window_in = 0;
        }
    }
    let t = Instant::now();
    let items = layer.finish();
    report.longest_feed_us = report
        .longest_feed_us
        .max(u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX));
    take(items, &mut report, &mut window);
    flush(&mut rec, start, window_in, &mut window)?;
    if let Some((kib, src)) = resident_kib() {
        let best = report.peak_rss.map_or(0, |(k, _)| k);
        report.peak_rss = Some((kib.max(best), src));
    }
    rec.finish()?;
    report.reads = r;
    report.elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    report.counters = layer.counters();
    Ok((report, header))
}

/// Write one aggregated log line: the window's bytes in and, per kind, the
/// total payload length of that kind's items.
fn flush<W: Write>(
    rec: &mut Recorder<W>,
    start: Instant,
    bytes_in: usize,
    window: &mut BTreeMap<&'static str, usize>,
) -> std::io::Result<()> {
    let items: Vec<(&str, usize)> = window.iter().map(|(k, v)| (*k, *v)).collect();
    window.clear();
    rec.read(
        u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        bytes_in,
        &items,
    )
}
