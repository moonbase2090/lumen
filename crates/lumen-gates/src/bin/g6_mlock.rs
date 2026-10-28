//! Gate G6: does `mlock` succeed for a key page under the default
//! `RLIMIT_MEMLOCK`, run as an ordinary user?
//!
//! Prints one line per observation and ends with `G6 PASS` or `G6 FAIL`. PASS
//! needs the lock to succeed, the page to be resident afterwards, and on
//! Linux the page to be excluded from core dumps.
//! Run it as a normal user with the limits the machine starts with; a run as
//! root or with a raised limit says nothing about the default.

use std::process::ExitCode;

use lumen_gates::ffi_mem::{Region, effective_uid, memlock_limit, page_size};

/// Most pages the headroom probe tries to lock (1 MiB at 4 KiB pages).
const PROBE_PAGES: usize = 256;

fn fmt_limit(v: Option<u64>) -> String {
    v.map_or_else(|| "unlimited".to_owned(), |n| format!("{n} bytes"))
}

fn main() -> ExitCode {
    let page = match page_size() {
        Ok(p) => p,
        Err(e) => {
            println!("G6 FAIL page size: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "G6 os={} arch={} page={page}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match memlock_limit() {
        Ok(l) => println!(
            "G6 RLIMIT_MEMLOCK soft={} hard={}",
            fmt_limit(l.soft),
            fmt_limit(l.hard)
        ),
        Err(e) => println!("G6 RLIMIT_MEMLOCK unreadable: {e}"),
    }

    let mut key_page = match Region::new(page) {
        Ok(r) => r,
        Err(e) => {
            println!("G6 FAIL mmap: {e}");
            return ExitCode::FAILURE;
        }
    };
    key_page.fill(0xA5);
    let locked = key_page.lock();
    match &locked {
        Ok(()) => println!("G6 key page mlock: ok"),
        Err(e) => println!("G6 key page mlock: FAILED {e}"),
    }
    let resident = key_page.resident_pages(page);
    match &resident {
        Ok(n) => println!("G6 key page resident after lock: {n} of 1"),
        Err(e) => println!("G6 key page resident: unreadable {e}"),
    }
    let dumpable = key_page.dont_dump();
    match &dumpable {
        Ok(()) => println!("G6 key page MADV_DONTDUMP: ok"),
        Err(e) => println!("G6 key page MADV_DONTDUMP: {e}"),
    }
    if let Err(e) = key_page.unlock() {
        println!("G6 key page munlock: {e}");
    }

    // Headroom: the largest block of whole pages this process can lock.
    let locked_pages = (1..=PROBE_PAGES)
        .rev()
        .find(|n| Region::new(page * n).is_ok_and(|r| r.lock().is_ok()))
        .unwrap_or(0);
    println!(
        "G6 headroom: {locked_pages} pages ({} bytes) lockable at once",
        locked_pages * page
    );

    let uid = effective_uid();
    println!("G6 euid={uid}");
    if uid == 0 {
        println!("G6 FAIL run as an ordinary user; root bypasses the limit");
        return ExitCode::FAILURE;
    }
    // The page must lock, be resident, and on Linux be excluded from dumps.
    let dump_ok = cfg!(not(target_os = "linux")) || dumpable.is_ok();
    if locked.is_ok() && matches!(resident, Ok(1)) && dump_ok {
        println!("G6 PASS");
        ExitCode::SUCCESS
    } else {
        println!("G6 FAIL");
        ExitCode::FAILURE
    }
}
