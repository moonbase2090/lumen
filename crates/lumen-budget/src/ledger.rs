//! The per-document accounting of the pooled resources, and the budget shares.

use crate::limits;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A resource with a document-wide budget that producers and the terminal share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resource {
    /// Blocks in the document.
    Blocks,
    /// Grids in the document.
    Grids,
    /// Grid cells in the document.
    GridCells,
    /// Scrollback, in bytes.
    ScrollbackBytes,
    /// The shaping cache, in bytes.
    ShapingCacheBytes,
    /// Decoded images that are live, in bytes.
    DecodedImageBytes,
    /// GPU textures, in bytes.
    GpuTextureBytes,
}

impl Resource {
    /// Every resource.
    pub const ALL: [Resource; 7] = [
        Resource::Blocks,
        Resource::Grids,
        Resource::GridCells,
        Resource::ScrollbackBytes,
        Resource::ShapingCacheBytes,
        Resource::DecodedImageBytes,
        Resource::GpuTextureBytes,
    ];

    /// The whole budget.
    #[must_use]
    pub const fn capacity(self) -> u64 {
        match self {
            Resource::Blocks => limits::BLOCKS_PER_DOCUMENT,
            Resource::Grids => limits::GRIDS_PER_DOCUMENT,
            Resource::GridCells => limits::GRID_CELLS_PER_DOCUMENT,
            Resource::ScrollbackBytes => limits::SCROLLBACK_BYTES,
            Resource::ShapingCacheBytes => limits::SHAPING_CACHE_BYTES,
            Resource::DecodedImageBytes => limits::DECODED_IMAGE_BYTES,
            Resource::GpuTextureBytes => limits::GPU_TEXTURE_BYTES,
        }
    }

    /// The name the vectors use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Resource::Blocks => "blocks",
            Resource::Grids => "grids",
            Resource::GridCells => "grid_cells",
            Resource::ScrollbackBytes => "scrollback_bytes",
            Resource::ShapingCacheBytes => "shaping_cache_bytes",
            Resource::DecodedImageBytes => "decoded_image_bytes",
            Resource::GpuTextureBytes => "gpu_texture_bytes",
        }
    }

    /// The resource with this vector name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Resource> {
        Resource::ALL.into_iter().find(|r| r.name() == name)
    }

    const fn index(self) -> usize {
        self as usize
    }

    /// The part kept for local use: a quarter, rounded up, so it is never less.
    const fn local_reserve(self) -> u64 {
        self.capacity().div_ceil(4)
    }

    /// What producers share between them: the budget less the local reserve.
    const fn remainder(self) -> u64 {
        self.capacity() - self.local_reserve()
    }

    /// The most one producer may hold: half of the remainder.
    const fn producer_cap(self) -> u64 {
        self.remainder() / 2
    }

    /// What all unsigned producers share: a quarter, rounded down.
    const fn unsigned_pool(self) -> u64 {
        self.capacity() / 4
    }
}

/// Who a reservation is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimant {
    /// The terminal itself.
    Local,
    /// A producer, whose output is `sealed` or not.
    Producer {
        /// The producer.
        id: u32,
        /// Whether its output is sealed. Unsigned producers share a quarter.
        sealed: bool,
    },
}

/// Why a reservation was refused. Checked in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// It would take the budget past its whole size, or the sum overflows.
    OverCapacity,
    /// Producers together would take into the quarter kept for local use.
    LocalReserve,
    /// The producer would hold more than half of what producers share.
    ProducerShare,
    /// An unsigned producer would hold more than its share of the unsigned quarter.
    UnsignedShare,
}

/// Why a ticket could not be released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseError {
    /// The ticket is from another ledger.
    Foreign,
    /// The ledger does not hold what the ticket says. A ticket from this
    /// ledger cannot cause this; it is checked so that a broken invariant is an
    /// error and never an underflow.
    Unaccounted,
}

static NEXT_LEDGER: AtomicU64 = AtomicU64::new(1);

/// What a reservation holds, until it is released. It is not `Clone`, so it
/// cannot be released twice.
#[derive(Debug)]
#[must_use = "a ticket that is dropped keeps its reservation until the ledger is gone"]
pub struct Ticket {
    ledger: u64,
    resource: Resource,
    claimant: Claimant,
    amount: u64,
}

impl Ticket {
    /// What the ticket holds.
    #[must_use]
    pub fn amount(&self) -> u64 {
        self.amount
    }
}

#[derive(Debug, Default)]
struct Pool {
    total: u64,
    producers: BTreeMap<u32, u64>,
    producer_total: u64,
    unsigned_total: u64,
}

/// The pooled resources of one document.
///
/// The shares apply to every resource alike, as integers:
///
/// - the terminal keeps a quarter of the budget, rounded up, that producers
///   cannot reach;
/// - one producer holds at most half of the rest, rounded down;
/// - unsigned producers hold at most a quarter of the budget, rounded down,
///   between them, and each at most that quarter divided by the visible panes.
///
/// A ledger is not `Clone`. A ticket is bound to the ledger that issued it, and a
/// copy would hold the same accounting under the same identity, so it would
/// accept the original's tickets and corrupt both.
#[derive(Debug)]
pub struct Ledger {
    id: u64,
    pools: [Pool; 7],
    panes: u32,
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

impl Ledger {
    /// An empty ledger with one visible pane.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: NEXT_LEDGER.fetch_add(1, Ordering::Relaxed),
            pools: Default::default(),
            panes: 1,
        }
    }

    /// The panes visible under a multiplexer. The unsigned quarter is split
    /// across them. Fewer than one counts as one.
    pub fn set_visible_panes(&mut self, panes: u32) {
        self.panes = panes.max(1);
    }

    /// Reserve `amount` of `resource` for `who`, before the thing it pays for is
    /// allocated.
    ///
    /// # Errors
    ///
    /// Returns the first rule the reservation breaks. Nothing is reserved then.
    pub fn reserve(
        &mut self,
        resource: Resource,
        who: Claimant,
        amount: u64,
    ) -> Result<Ticket, Denied> {
        let pool = &self.pools[resource.index()];
        let total = pool
            .total
            .checked_add(amount)
            .filter(|t| *t <= resource.capacity())
            .ok_or(Denied::OverCapacity)?;
        if let Claimant::Producer { id, sealed } = who {
            let held = pool.producers.get(&id).copied().unwrap_or(0);
            let producers = pool
                .producer_total
                .checked_add(amount)
                .filter(|t| *t <= resource.remainder())
                .ok_or(Denied::LocalReserve)?;
            let mine = held
                .checked_add(amount)
                .filter(|t| *t <= resource.producer_cap())
                .ok_or(Denied::ProducerShare)?;
            if !sealed {
                let pool_ok = pool
                    .unsigned_total
                    .checked_add(amount)
                    .is_some_and(|t| t <= resource.unsigned_pool());
                let pane_ok = mine <= resource.unsigned_pool() / u64::from(self.panes);
                if !(pool_ok && pane_ok) {
                    return Err(Denied::UnsignedShare);
                }
            }
            let pool = &mut self.pools[resource.index()];
            pool.producer_total = producers;
            pool.producers.insert(id, mine);
            if !sealed {
                pool.unsigned_total += amount;
            }
        }
        self.pools[resource.index()].total = total;
        Ok(Ticket {
            ledger: self.id,
            resource,
            claimant: who,
            amount,
        })
    }

    /// Give back what a ticket holds.
    ///
    /// # Errors
    ///
    /// Returns an error for a ticket from another ledger, and releases nothing.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the ticket is consumed so that it cannot be released twice"
    )]
    pub fn release(&mut self, ticket: Ticket) -> Result<(), ReleaseError> {
        if ticket.ledger != self.id {
            return Err(ReleaseError::Foreign);
        }
        let pool = &mut self.pools[ticket.resource.index()];
        let amount = ticket.amount;
        // Work out every new figure first, so a failure changes nothing.
        let total = pool
            .total
            .checked_sub(amount)
            .ok_or(ReleaseError::Unaccounted)?;
        if let Claimant::Producer { id, sealed } = ticket.claimant {
            let producer_total = pool
                .producer_total
                .checked_sub(amount)
                .ok_or(ReleaseError::Unaccounted)?;
            let unsigned_total = if sealed {
                pool.unsigned_total
            } else {
                pool.unsigned_total
                    .checked_sub(amount)
                    .ok_or(ReleaseError::Unaccounted)?
            };
            let left = pool
                .producers
                .get(&id)
                .copied()
                .unwrap_or(0)
                .checked_sub(amount)
                .ok_or(ReleaseError::Unaccounted)?;
            pool.producer_total = producer_total;
            pool.unsigned_total = unsigned_total;
            if left == 0 {
                pool.producers.remove(&id);
            } else {
                pool.producers.insert(id, left);
            }
        }
        pool.total = total;
        Ok(())
    }

    /// How much of `resource` is reserved, by anyone.
    #[must_use]
    pub fn used(&self, resource: Resource) -> u64 {
        self.pools[resource.index()].total
    }

    /// How much of `resource` producer `id` holds.
    #[must_use]
    pub fn used_by_producer(&self, resource: Resource, id: u32) -> u64 {
        self.pools[resource.index()]
            .producers
            .get(&id)
            .copied()
            .unwrap_or(0)
    }
}
