//! Memory-locking primitives for gate G6.
//!
//! The one FFI module in this crate. The wrappers own every raw pointer: a
//! [`Region`] is an anonymous mapping that is unmapped on drop, and nothing
//! hands out its address.

use std::io;

/// Soft and hard `RLIMIT_MEMLOCK`, in bytes. `None` means unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemlockLimit {
    /// Soft limit, the one `mlock` is checked against.
    pub soft: Option<u64>,
    /// Hard limit.
    pub hard: Option<u64>,
}

#[allow(
    clippy::useless_conversion,
    reason = "rlim_t is u64 on 64-bit targets only"
)]
fn limit(value: libc::rlim_t) -> Option<u64> {
    if value == libc::RLIM_INFINITY {
        None
    } else {
        Some(u64::from(value))
    }
}

/// The effective user id of this process.
#[must_use]
pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments, has no memory effects and cannot fail.
    unsafe { libc::geteuid() }
}

/// Read the process's `RLIMIT_MEMLOCK`.
///
/// # Errors
///
/// Returns the OS error if `getrlimit` fails.
pub fn memlock_limit() -> io::Result<MemlockLimit> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rl` is a valid, writable `rlimit` for the whole call.
    let rc = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &raw mut rl) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(MemlockLimit {
        soft: limit(rl.rlim_cur),
        hard: limit(rl.rlim_max),
    })
}

/// The system page size in bytes.
///
/// # Errors
///
/// Returns the OS error if `sysconf` fails.
pub fn page_size() -> io::Result<usize> {
    // SAFETY: `sysconf` takes an integer name and has no memory effects.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(n).map_err(|_| io::Error::last_os_error())
}

/// An anonymous, private, page-aligned mapping.
#[derive(Debug)]
pub struct Region {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Region {
    /// Map `len` bytes (a multiple of the page size), zero-filled.
    ///
    /// # Errors
    ///
    /// Returns the OS error if `mmap` fails.
    pub fn new(len: usize) -> io::Result<Self> {
        // SAFETY: an anonymous private mapping with a null hint touches no
        // existing memory; the result is checked before use.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { ptr, len })
    }

    /// Fill the mapping with `byte`, which also makes every page resident.
    pub fn fill(&mut self, byte: u8) {
        // SAFETY: `ptr` is a live writable mapping of exactly `len` bytes, and
        // `&mut self` means nothing else is reading or writing it.
        unsafe { std::ptr::write_bytes(self.ptr.cast::<u8>(), byte, self.len) };
    }

    /// Lock the mapping into RAM.
    ///
    /// # Errors
    ///
    /// Returns the OS error from `mlock`, for example `ENOMEM` when the limit
    /// is exceeded.
    pub fn lock(&self) -> io::Result<()> {
        // SAFETY: `ptr` and `len` describe this live mapping.
        if unsafe { libc::mlock(self.ptr, self.len) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Release the lock.
    ///
    /// # Errors
    ///
    /// Returns the OS error from `munlock`.
    pub fn unlock(&self) -> io::Result<()> {
        // SAFETY: `ptr` and `len` describe this live mapping.
        if unsafe { libc::munlock(self.ptr, self.len) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Count the pages that are resident in RAM.
    ///
    /// # Errors
    ///
    /// Returns the OS error from `mincore`.
    pub fn resident_pages(&self, page: usize) -> io::Result<usize> {
        let mut vec = vec![0 as libc::c_char; self.len.div_ceil(page)];
        // SAFETY: `vec` has one byte per page of the mapping, as `mincore`
        // requires, and the mapping is live.
        let rc = unsafe { libc::mincore(self.ptr, self.len, vec.as_mut_ptr().cast()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(vec.iter().filter(|b| **b & 1 != 0).count())
    }

    /// Exclude the mapping from core dumps (Linux only).
    ///
    /// # Errors
    ///
    /// Returns the OS error from `madvise`, or `Unsupported` off Linux.
    #[cfg(target_os = "linux")]
    pub fn dont_dump(&self) -> io::Result<()> {
        // SAFETY: `ptr` and `len` describe this live mapping.
        if unsafe { libc::madvise(self.ptr, self.len, libc::MADV_DONTDUMP) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Exclude the mapping from core dumps (Linux only).
    ///
    /// # Errors
    ///
    /// Always `Unsupported` off Linux.
    #[cfg(not(target_os = "linux"))]
    #[allow(clippy::unused_self, reason = "keeps one signature on every platform")]
    pub fn dont_dump(&self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `len` came from a successful `mmap` and are
        // unmapped exactly once, here.
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_page_is_resident_and_unlock_succeeds() {
        let page = page_size().unwrap_or(4096);
        let made = Region::new(page);
        assert!(made.is_ok(), "mmap failed");
        let Ok(mut region) = made else { return };
        region.fill(0xA5);
        assert!(
            region.lock().is_ok(),
            "one page must lock under the test runner's limit"
        );
        assert_eq!(region.resident_pages(page).ok(), Some(1));
        assert!(region.unlock().is_ok());
    }

    #[test]
    fn locking_more_than_a_finite_limit_fails() {
        let page = page_size().unwrap_or(4096);
        let made = memlock_limit();
        assert!(made.is_ok(), "getrlimit failed");
        let Ok(limit) = made else { return };
        let Some(soft) = limit.soft else {
            return; // unlimited: nothing to exceed
        };
        if effective_uid() == 0 {
            return; // root ignores the limit
        }
        let too_big = usize::try_from(soft)
            .unwrap_or(usize::MAX / 2)
            .saturating_add(page * 2);
        let made = Region::new(too_big.div_ceil(page) * page);
        assert!(made.is_ok(), "mmap failed");
        let Ok(region) = made else { return };
        assert!(region.lock().is_err());
    }

    #[test]
    fn page_size_is_a_power_of_two() {
        let page = page_size().unwrap_or(0);
        assert!(page.is_power_of_two() && page >= 4096);
    }

    #[test]
    fn effective_uid_matches_the_environment_report() {
        let out = std::process::Command::new("id").arg("-u").output();
        let reported = out.ok().and_then(|o| String::from_utf8(o.stdout).ok());
        assert_eq!(
            reported.map(|s| s.trim().to_owned()),
            Some(effective_uid().to_string())
        );
    }
}
