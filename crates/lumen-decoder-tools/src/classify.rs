//! Crash classifier for libFuzzer logs.

/// What kind of crash a log reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A unit ran past the fuzzer's timeout (2 s in the campaign).
    Hang,
    /// Memory use went over the fuzzer's RSS limit (512 MiB in the campaign).
    Memory,
    /// An AddressSanitizer, UBSan or similar report.
    Sanitizer,
    /// A Rust panic or abort with no sanitizer report.
    Panic,
    /// A crash the fuzzer reported that none of the above explains.
    Other,
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
        }
    }
}

/// Classifies a log. `None` means the log reports no crash.
///
/// A sanitizer report wins over the fuzzer's own line: libFuzzer prints
/// `ERROR: libFuzzer: deadly signal` after an ASan report too.
#[must_use]
pub fn classify(log: &str) -> Option<Kind> {
    let has = |needle: &str| log.contains(needle);
    if has("ERROR: libFuzzer: timeout") {
        Some(Kind::Hang)
    } else if has("ERROR: libFuzzer: out-of-memory")
        || (has("memory allocation of") && has("bytes failed"))
    {
        Some(Kind::Memory)
    } else if has("ERROR: AddressSanitizer") || has("ERROR: LeakSanitizer") || has("runtime error:") || has("ERROR: MemorySanitizer") {
        Some(Kind::Sanitizer)
    } else if has("panicked at") {
        Some(Kind::Panic)
    } else if has("ERROR: libFuzzer:") {
        Some(Kind::Other)
    } else {
        None
    }
}
