//! Whether the system asks for less motion.
//!
//! macOS has "Reduce motion" in Accessibility, and a GNOME desktop turns
//! animations off with its `enable-animations` setting. When either says so the
//! terminal stops easing the wheel's scrolling and stops blinking the cursor:
//! a scroll jumps and a blinking cursor stays on. The answer comes from a fixed
//! system program started with fixed arguments (the same way the notification
//! backend does it, never a shell), and anything unexpected, such as a missing
//! program, an error or unrecognised text, means motion is left as it is.
//! Windows has no answer here yet, so motion stays as it was there.

use std::process::{Command, Stdio};

/// The macOS Accessibility preference: `1` when Reduce motion is on.
const MACOS_READ: (&str, [&str; 3]) = (
    "/usr/bin/defaults",
    ["read", "com.apple.Accessibility", "ReduceMotionEnabled"],
);

/// The GNOME setting: `true` while animations are on.
const GNOME_READ: (&str, [&str; 3]) = (
    "/usr/bin/gsettings",
    ["get", "org.gnome.desktop.interface", "enable-animations"],
);

/// Whether `defaults read com.apple.Accessibility ReduceMotionEnabled` said
/// Reduce motion is on. A missing key makes `defaults` fail, which is off.
#[must_use]
pub fn parse_macos(output: &str) -> bool {
    output.trim() == "1"
}

/// Whether `gsettings get ... enable-animations` said animations are off.
#[must_use]
pub fn parse_gnome(output: &str) -> bool {
    output.trim() == "false"
}

fn run((program, args): (&str, [&str; 3])) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether the system is set to reduce motion right now.
#[must_use]
pub fn system_reduces_motion() -> bool {
    match std::env::consts::OS {
        "macos" => run(MACOS_READ).is_some_and(|o| parse_macos(&o)),
        "linux" => run(GNOME_READ).is_some_and(|o| parse_gnome(&o)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_one_is_reduced_and_anything_else_is_not() {
        assert!(parse_macos("1\n"));
        assert!(!parse_macos("0\n"));
        assert!(!parse_macos(""));
        assert!(!parse_macos("Error: key not found"));
    }

    #[test]
    fn gnome_false_is_reduced_and_anything_else_is_not() {
        assert!(parse_gnome("false\n"));
        assert!(!parse_gnome("true\n"));
        assert!(!parse_gnome(""));
        assert!(!parse_gnome("No such schema"));
    }
}
