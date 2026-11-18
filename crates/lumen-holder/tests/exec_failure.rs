//! A holder that cannot start is reported as an error, even when the lowest
//! free descriptors are the ones the holder's four are placed on.
//!
//! The standard library tells the parent that an exec failed over a socket it
//! opens just before the fork. That socket takes the lowest free descriptor.
//! The spawner places the holder's descriptors at 3 to 6 between the fork and
//! the exec, so a socket that landed on one of those numbers was overwritten
//! and the failure was never reported: the start looked like a success.
//!
//! The layout that triggers it needs exact descriptor numbers, so the scenario
//! runs in a copy of this test binary that has closed every descriptor and has
//! no other test running; the first test starts it and reads what it wrote.

#![cfg(target_os = "linux")]

use lumen_holder::ffi_linux::{close_all_except, pidfd_open};
use lumen_holder::spawn::{Channels, start_on};
use std::path::Path;

const SCENARIO: &str =
    "a_program_that_cannot_run_is_an_error_when_descriptors_3_to_6_are_free_scenario";
const RESULT_ENV: &str = "LUMEN_HOLDER_EXEC_FAILURE_RESULT";

#[test]
fn a_program_that_cannot_run_is_an_error_when_descriptors_3_to_6_are_free() {
    let dir = std::env::temp_dir().join(format!("lumen-exec-failure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let result = dir.join("result");
    let exe = std::env::current_exe().unwrap_or_else(|e| unreachable!("{e}"));
    let status = std::process::Command::new(exe)
        .args(["--exact", SCENARIO, "--test-threads=1"])
        .env(RESULT_ENV, &result)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let said = std::fs::read_to_string(&result).unwrap_or_default();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        status.success(),
        "the scenario did not finish: {status}, {said:?}"
    );
    assert_eq!(
        said, "NotFound",
        "the missing program was not reported as one"
    );
}

/// Run only by the test above, in its own process.
#[test]
fn a_program_that_cannot_run_is_an_error_when_descriptors_3_to_6_are_free_scenario() {
    let Some(result) = std::env::var_os(RESULT_ENV) else {
        return;
    };
    // Close everything, fill the low descriptors, make what the spawn needs
    // above them, then free the low ones: the standard library's four standard
    // streams take 0 to 3, and its exec-failure socket takes 4 and 5.
    close_all_except(&[]).unwrap_or_else(|e| unreachable!("{e}"));
    let fillers: Vec<std::fs::File> = (0..30)
        .map(|_| std::fs::File::open("/dev/null").unwrap_or_else(|e| unreachable!("{e}")))
        .collect();
    let mut channels = Channels::new().unwrap_or_else(|e| unreachable!("{e}"));
    let _shell_end = channels
        .take_shell_end()
        .unwrap_or_else(|| unreachable!("the shell end is there"));
    let pidfd = pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("pidfd_open: {e}"));
    drop(fillers);

    let started = start_on(
        channels,
        Path::new("/nonexistent/lumen-holder"),
        &pidfd,
        std::process::id(),
        std::process::id(),
        &[],
    );
    let said = match started {
        Ok(_) => "started".to_owned(),
        Err(e) => format!("{:?}", e.kind()),
    };
    std::fs::write(result, said).unwrap_or_else(|e| unreachable!("{e}"));
}

const PATH_SCENARIO: &str = "a_bare_program_name_is_looked_up_on_path_scenario";

#[test]
fn a_bare_program_name_is_looked_up_on_path() {
    let dir = std::env::temp_dir().join(format!("lumen-exec-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("/bin/true", dir.join("lumen-holder-probe"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let result = dir.join("result");
    let exe = std::env::current_exe().unwrap_or_else(|e| unreachable!("{e}"));
    let status = std::process::Command::new(exe)
        .args(["--exact", PATH_SCENARIO, "--test-threads=1"])
        .env(RESULT_ENV, &result)
        .env("PATH", &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let said = std::fs::read_to_string(&result).unwrap_or_default();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        status.success(),
        "the scenario did not finish: {status}, {said:?}"
    );
    // The name in the directory on PATH runs; one that is not there is not found.
    assert_eq!(said, "started NotFound");
}

/// Run only by the test above, in its own process, with `PATH` set to one
/// directory.
#[test]
fn a_bare_program_name_is_looked_up_on_path_scenario() {
    let Some(result) = std::env::var_os(RESULT_ENV) else {
        return;
    };
    let start = |name: &str| {
        let mut channels = Channels::new().unwrap_or_else(|e| unreachable!("{e}"));
        let _shell_end = channels
            .take_shell_end()
            .unwrap_or_else(|| unreachable!("the shell end is there"));
        let pidfd =
            pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("pidfd_open: {e}"));
        start_on(
            channels,
            Path::new(name),
            &pidfd,
            std::process::id(),
            std::process::id(),
            &[],
        )
    };
    let found = match start("lumen-holder-probe") {
        Ok(mut running) => {
            let _ = running.child.wait();
            "started".to_owned()
        }
        Err(e) => format!("{:?}", e.kind()),
    };
    let missing = match start("lumen-holder-absent") {
        Ok(_) => "started".to_owned(),
        Err(e) => format!("{:?}", e.kind()),
    };
    std::fs::write(result, format!("{found} {missing}")).unwrap_or_else(|e| unreachable!("{e}"));
}
