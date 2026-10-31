//! Random sequences of reservations and releases: whatever happens, the budget
//! is never over its size, producers never reach the local reserve, and
//! releasing everything leaves nothing.

use lumen_budget::{Claimant, Ledger, Resource, Ticket};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn claimant(rng: &mut Rng) -> Claimant {
    match rng.below(6) {
        0 => Claimant::Local,
        n @ 1..=3 => Claimant::Producer {
            id: u32::try_from(n).unwrap_or(1),
            sealed: true,
        },
        n => Claimant::Producer {
            id: u32::try_from(n).unwrap_or(4),
            sealed: false,
        },
    }
}

/// An amount that is often small and sometimes a large part of the budget.
fn amount(rng: &mut Rng, capacity: u64) -> u64 {
    match rng.below(4) {
        0 => rng.below(8),
        1 => rng.below(capacity / 16 + 1),
        2 => rng.below(capacity / 3 + 1),
        _ => rng.below(capacity + 2),
    }
}

#[test]
fn the_shares_hold_under_any_sequence() {
    let mut accepted_total = 0u64;
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut ledger = Ledger::new();
        ledger.set_visible_panes(u32::try_from(1 + rng.below(4)).unwrap_or(1));
        let mut held: Vec<Ticket> = Vec::new();
        for _ in 0..400 {
            let resource = Resource::ALL[usize::try_from(rng.below(7)).unwrap_or(0)];
            let capacity = resource.capacity();
            if rng.below(3) == 0 && !held.is_empty() {
                let i = usize::try_from(rng.below(held.len() as u64)).unwrap_or(0);
                assert!(ledger.release(held.swap_remove(i)).is_ok());
            } else {
                let who = claimant(&mut rng);
                let a = amount(&mut rng, capacity);
                if let Ok(ticket) = ledger.reserve(resource, who, a) {
                    accepted_total += 1;
                    held.push(ticket);
                }
            }
            // The invariants, for every resource, after every step.
            for r in Resource::ALL {
                let cap = r.capacity();
                let producers: u64 = (0..8).map(|id| ledger.used_by_producer(r, id)).sum();
                assert!(ledger.used(r) <= cap, "seed {seed}: {r:?} over capacity");
                assert!(
                    producers <= cap - cap.div_ceil(4),
                    "seed {seed}: producers reached the local reserve of {r:?}"
                );
                assert!(
                    ledger.used(r) >= producers,
                    "seed {seed}: producers hold more than is used of {r:?}"
                );
                for id in 0..8 {
                    assert!(
                        ledger.used_by_producer(r, id) <= (cap - cap.div_ceil(4)) / 2,
                        "seed {seed}: producer {id} over its share of {r:?}"
                    );
                }
            }
        }
        for ticket in held {
            assert!(ledger.release(ticket).is_ok());
        }
        for r in Resource::ALL {
            assert_eq!(
                ledger.used(r),
                0,
                "seed {seed}: {r:?} not empty after releasing everything"
            );
        }
    }
    assert!(
        accepted_total > 10_000,
        "{accepted_total} reservations were accepted"
    );
}

#[test]
fn the_terminal_can_always_take_its_reserve_when_it_holds_nothing() {
    for seed in 1..=100u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1);
        let mut ledger = Ledger::new();
        let mut held = Vec::new();
        for r in Resource::ALL {
            // Producers try as hard as they can.
            for _ in 0..200 {
                let who = Claimant::Producer {
                    id: u32::try_from(rng.below(6)).unwrap_or(0),
                    sealed: rng.below(2) == 0,
                };
                if let Ok(t) = ledger.reserve(r, who, rng.below(r.capacity() / 4 + 1)) {
                    held.push(t);
                }
            }
            let reserve = r.capacity().div_ceil(4);
            let local = ledger.reserve(r, Claimant::Local, reserve);
            assert!(
                local.is_ok(),
                "seed {seed}: the local reserve of {r:?} was reached"
            );
            held.extend(local);
        }
    }
}

#[test]
fn a_ticket_from_another_ledger_is_refused() {
    let mut a = Ledger::new();
    let mut b = Ledger::new();
    let Ok(ticket) = a.reserve(Resource::Blocks, Claimant::Local, 5) else {
        unreachable!("a small reservation is granted")
    };
    assert!(b.release(ticket).is_err());
    assert_eq!(b.used(Resource::Blocks), 0);
    assert_eq!(a.used(Resource::Blocks), 5);
}
