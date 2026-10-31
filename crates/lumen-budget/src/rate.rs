//! Per-second limits, in sliding windows.

use crate::limits;
use std::collections::{BTreeMap, VecDeque};

/// A limit on how much may be taken in any window of [`limits::WINDOW_MS`].
///
/// The window slides: an amount counts from the millisecond it was taken until
/// the window has passed it, not until a second boundary. A clock that goes
/// backwards counts as no time passing.
#[derive(Debug, Clone)]
pub struct RateLimit {
    limit: u64,
    taken: VecDeque<(u64, u64)>,
    total: u64,
    last_ms: u64,
}

impl RateLimit {
    /// A limit of `limit` per window.
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            taken: VecDeque::new(),
            total: 0,
            last_ms: 0,
        }
    }

    fn prune(&mut self, now_ms: u64) {
        self.last_ms = self.last_ms.max(now_ms);
        while let Some(&(t, n)) = self.taken.front() {
            if self.last_ms.saturating_sub(t) < limits::WINDOW_MS {
                break;
            }
            self.total -= n;
            self.taken.pop_front();
        }
    }

    /// Whether `n` more would fit at `now_ms`.
    pub fn fits(&mut self, now_ms: u64, n: u64) -> bool {
        self.prune(now_ms);
        self.total.checked_add(n).is_some_and(|t| t <= self.limit)
    }

    /// Take `n` at `now_ms` if it fits. A refused amount is not counted.
    pub fn try_take(&mut self, now_ms: u64, n: u64) -> bool {
        if !self.fits(now_ms, n) {
            return false;
        }
        if n > 0 {
            self.taken.push_back((self.last_ms, n));
            self.total += n;
        }
        true
    }
}

/// The limits on one producer: frames and bytes per second. An unsigned producer
/// has a quarter of each, split across the visible panes under a multiplexer.
#[derive(Debug, Clone)]
pub struct ProducerRates {
    frames: RateLimit,
    bytes: RateLimit,
}

impl ProducerRates {
    /// The limits of a producer that is `sealed` or not, in a terminal with
    /// `panes` visible panes. At least one pane is assumed.
    #[must_use]
    pub fn new(sealed: bool, panes: u32) -> Self {
        let share = |full: u64| {
            if sealed {
                full
            } else {
                full / 4 / u64::from(panes.max(1))
            }
        };
        Self {
            frames: RateLimit::new(share(limits::FRAMES_PER_PRODUCER_PER_SEC)),
            bytes: RateLimit::new(share(limits::BYTES_PER_PRODUCER_PER_SEC)),
        }
    }

    /// Admit one frame of `bytes` at `now_ms`. Both limits must have room, and
    /// nothing is counted if either has not.
    pub fn admit_frame(&mut self, now_ms: u64, bytes: u64) -> bool {
        if !(self.frames.fits(now_ms, 1) && self.bytes.fits(now_ms, bytes)) {
            return false;
        }
        self.frames.try_take(now_ms, 1) && self.bytes.try_take(now_ms, bytes)
    }
}

/// The patch limits of a document: per block and for the whole document.
#[derive(Debug, Clone)]
pub struct DocumentRates {
    document: RateLimit,
    blocks: BTreeMap<u64, RateLimit>,
}

impl Default for DocumentRates {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentRates {
    /// Limits with nothing taken.
    #[must_use]
    pub fn new() -> Self {
        Self {
            document: RateLimit::new(limits::PATCHES_PER_DOCUMENT_PER_SEC),
            blocks: BTreeMap::new(),
        }
    }

    /// Admit `n` patches to `block` at `now_ms`. Both the block's limit and the
    /// document's must have room, and nothing is counted if either has not.
    pub fn admit_patches(&mut self, now_ms: u64, block: u64, n: u64) -> bool {
        let per_block = self
            .blocks
            .entry(block)
            .or_insert_with(|| RateLimit::new(limits::PATCHES_PER_BLOCK_PER_SEC));
        if !(per_block.fits(now_ms, n) && self.document.fits(now_ms, n)) {
            return false;
        }
        per_block.try_take(now_ms, n) && self.document.try_take(now_ms, n)
    }

    /// Forget a block that is gone, so the table does not outlive it.
    pub fn forget_block(&mut self, block: u64) {
        self.blocks.remove(&block);
    }
}
