//! Crash classifier for libFuzzer logs.

/// What kind of crash a log reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A unit ran past the fuzzer's timeout (2 s in the campaign).
    Hang,
    /// Memory use went over the fuzzer's RSS limit (512 MiB in the campaign).
    Memory,
    /// An `AddressSanitizer`, `UBSan` or similar report.
    Sanitizer,
    /// A Rust panic or abort with no sanitizer report.
    Panic,
    /// A crash the fuzzer reported that none of the above explains.
    Other,
    /// An out-of-memory report from the fuzzer whose replay through the real
    /// worker stayed under the limit: an artifact of the sanitizer and the
    /// corpus, not a crash. It is recorded, with its replay size, and not counted.
    AsanArtifact,
}

impl Kind {
    /// The word written to the ledger.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Hang => "hang",
            Self::Memory => "memory",
            Self::Sanitizer => "sanitizer",
            Self::Panic => "panic",
            Self::Other => "other",
            Self::AsanArtifact => "asan-artifact",
        }
    }
}

/// Classifies a log. `None` means the log reports no crash.
///
/// A sanitizer report wins over the fuzzer's own line: libFuzzer prints
/// `ERROR: libFuzzer: deadly signal` after an `ASan` report too.
#[must_use]
pub fn classify(log: &str) -> Option<Kind> {
    let has = |needle: &str| log.contains(needle);
    if has("ERROR: libFuzzer: timeout") {
        Some(Kind::Hang)
    } else if has("ERROR: libFuzzer: out-of-memory")
        || (has("memory allocation of") && has("bytes failed"))
    {
        Some(Kind::Memory)
    } else if has("ERROR: AddressSanitizer")
        || has("ERROR: LeakSanitizer")
        || has("runtime error:")
        || has("ERROR: MemorySanitizer")
    {
        Some(Kind::Sanitizer)
    } else if has("panicked at") {
        Some(Kind::Panic)
    } else if has("ERROR: libFuzzer:") {
        Some(Kind::Other)
    } else {
        None
    }
}

/// The worker's memory limit in KiB.
const LIMIT_KIB: u64 = 512 * 1024;
/// `SIGABRT` as `subprocess` reports a death by signal: the capped allocator
/// aborts the worker when a decode wants more than the limit.
const SIGABRT_EXIT: i64 = -6;

/// Classifies a replay record (`scripts/decoder-replay.py`) of an `oom-*`
/// artifact: it is a memory crash if the real worker peaked over 512 MiB or
/// was aborted by its own allocator cap, and otherwise an [`Kind::AsanArtifact`].
/// `None` if the record has no peak size.
#[must_use]
pub fn classify_replay(record: &serde_json::Value) -> Option<Kind> {
    let peak = record["peak_rss_kb"].as_u64()?;
    let aborted = record["exit"].as_i64() == Some(SIGABRT_EXIT);
    Some(if peak > LIMIT_KIB || aborted {
        Kind::Memory
    } else {
        Kind::AsanArtifact
    })
}
