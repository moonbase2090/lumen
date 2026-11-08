//! The Windows start path on its own, so a failure names the step that failed
//! instead of the runner's bare "could not start".

#![cfg(windows)]
#![allow(missing_docs)]

use std::path::Path;

use lumen_decoder::ffi_win_spawn::spawn;

const WORKER: &str = env!("CARGO_BIN_EXE_lumen-decoder-worker");

#[test]
fn the_worker_starts_in_the_container_and_the_job() {
    let started = spawn(Path::new(WORKER), &[]);
    let Ok(s) = started else {
        let e = started.err();
        unreachable!("spawn failed: {e:?}");
    };
    // No input: the worker reads EOF and answers malformed, then exits.
    drop(s.stdin);
    let t = std::time::Instant::now();
    let worker = s.worker;
    while worker.try_wait().ok().flatten().is_none() && t.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        worker.try_wait().ok().flatten().is_some(),
        "the worker did not exit"
    );
}
