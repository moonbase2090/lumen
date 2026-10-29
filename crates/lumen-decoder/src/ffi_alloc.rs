//! A global allocator that refuses to hold more than a fixed number of live
//! bytes. It backs the 512 MiB limit where the platform will not enforce an
//! address-space limit (macOS refuses to set one), and it applies on Linux as
//! well, so both platforms stop at the same point.
//!
//! It counts the Rust heap only. Memory mapped outside the allocator is not
//! counted; the worker does none.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The system allocator with a cap on live bytes.
#[derive(Debug)]
pub struct CappedAlloc {
    live: AtomicUsize,
    cap: usize,
}

impl CappedAlloc {
    /// An allocator that allows at most `cap` live bytes.
    #[must_use]
    pub const fn new(cap: usize) -> Self {
        Self {
            live: AtomicUsize::new(0),
            cap,
        }
    }

    fn take(&self, n: usize) -> bool {
        let mut cur = self.live.load(Ordering::Relaxed);
        loop {
            let Some(next) = cur.checked_add(n).filter(|&v| v <= self.cap) else {
                return false;
            };
            match self
                .live
                .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return true,
                Err(seen) => cur = seen,
            }
        }
    }

    fn give(&self, n: usize) {
        self.live.fetch_sub(n, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the caller's layout, and the
// byte count is kept in step: bytes are taken before an allocation and given
// back if it fails, and given back on every successful free. A refused
// allocation returns null, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for CappedAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.take(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            self.give(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !self.take(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            self.give(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from a matching allocation, as the caller guarantees.
        unsafe { System.dealloc(ptr, layout) };
        self.give(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !self.take(new_size - old) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                self.give(new_size - old);
            }
        } else if new_size < old {
            self.give(old - new_size);
        }
        p
    }
}
