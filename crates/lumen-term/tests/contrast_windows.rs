//! On a real Windows machine the system's answer about High Contrast can be
//! read, and it is the answer the system gives: the read is compared with the
//! system's own report (`SystemParameters.HighContrast`, asked of the .NET
//! runtime in a separate process), so it holds whether or not a contrast theme
//! is on where the suite runs.

#![cfg(windows)]

use std::os::windows::process::CommandExt as _;
use std::process::Command;

#[test]
fn the_read_matches_what_the_system_reports_about_high_contrast() {
    let theirs = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName PresentationFramework; [System.Windows.SystemParameters]::HighContrast",
        ])
        // CREATE_NO_WINDOW
        .creation_flags(0x0800_0000)
        .output()
        .unwrap_or_else(|e| unreachable!("powershell starts: {e}"));
    let said = String::from_utf8_lossy(&theirs.stdout).trim().to_owned();
    let want = match said.as_str() {
        "True" => true,
        "False" => false,
        other => unreachable!("the system said {other:?}"),
    };
    let ours = lumen_term::contrast::system_increases_contrast();
    println!("the system says {want}; system_increases_contrast() = {ours:?}");
    assert_eq!(ours, Some(want));
}
