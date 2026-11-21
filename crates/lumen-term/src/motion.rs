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

/// The preference behind macOS's Reduce motion: `1` when it is on. The key is
/// absent until it has been turned on once, which makes `defaults` fail.
const MACOS_READ: (&str, [&str; 3]) = (
    "/usr/bin/defaults",
    ["read", "com.apple.universalaccess", "reduceMotion"],
);

/// The GNOME setting: `true` while animations are on.
const GNOME_READ: (&str, [&str; 3]) = (
    "/usr/bin/gsettings",
    ["get", "org.gnome.desktop.interface", "enable-animations"],
);

/// What `defaults` says when the key has never been written.
const MACOS_ABSENT: &str =
    "The domain/default pair of (com.apple.universalaccess, reduceMotion) does not exist";

/// What `defaults read com.apple.universalaccess reduceMotion` said. `1` is on
/// and `0` is off. The key is absent until Reduce motion has been turned on
/// once, and then `defaults` fails saying that very pair does not exist: that
/// is off. Any other failure (including the same words about another domain
/// or key), or text that is not `0` or `1`, is no answer.
#[must_use]
pub fn macos_answer(succeeded: bool, stdout: &str, stderr: &str) -> Option<bool> {
    if succeeded {
        match stdout.trim() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        }
    } else {
        stderr.contains(MACOS_ABSENT).then_some(false)
    }
}

/// What `gsettings get ... enable-animations` said: `false` means animations
/// are off, which is reduced motion. Anything else but `true` is no answer.
#[must_use]
pub fn gnome_answer(succeeded: bool, stdout: &str) -> Option<bool> {
    if !succeeded {
        return None;
    }
    match stdout.trim() {
        "false" => Some(true),
        "true" => Some(false),
        _ => None,
    }
}

fn run((program, args): (&str, [&str; 3])) -> Option<(bool, String, String)> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Whether the system is set to reduce motion right now, or `None` when it
/// cannot be told (a failed read, a program that is missing, a system with no
/// answer here). A caller keeps what it last knew on `None`.
#[must_use]
pub fn system_reduces_motion() -> Option<bool> {
    match std::env::consts::OS {
        "macos" => {
            let (ok, out, err) = run(MACOS_READ)?;
            macos_answer(ok, &out, &err)
        }
        "linux" => {
            let (ok, out, _) = run(GNOME_READ)?;
            gnome_answer(ok, &out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_one_is_on_zero_is_off_and_an_absent_key_is_off() {
        assert_eq!(macos_answer(true, "1\n", ""), Some(true));
        assert_eq!(macos_answer(true, "0\n", ""), Some(false));
        let absent =
            "The domain/default pair of (com.apple.universalaccess, reduceMotion) does not exist\n";
        assert_eq!(macos_answer(false, "", absent), Some(false));
    }

    #[test]
    fn a_macos_read_that_failed_another_way_or_said_something_else_is_no_answer() {
        assert_eq!(macos_answer(false, "", "Operation not permitted"), None);
        let other = "The domain/default pair of (com.example.other, reduceMotion) does not exist\n";
        assert_eq!(macos_answer(false, "", other), None);
        let file = "/var/prefs/com.apple.universalaccess.plist does not exist\n";
        assert_eq!(macos_answer(false, "", file), None);
        assert_eq!(macos_answer(false, "", ""), None);
        assert_eq!(macos_answer(true, "", ""), None);
        assert_eq!(macos_answer(true, "2\n", ""), None);
        assert_eq!(macos_answer(true, "Error: key not found", ""), None);
    }

    #[test]
    fn gnome_false_is_reduced_true_is_not_and_the_rest_is_no_answer() {
        assert_eq!(gnome_answer(true, "false\n"), Some(true));
        assert_eq!(gnome_answer(true, "true\n"), Some(false));
        assert_eq!(gnome_answer(true, ""), None);
        assert_eq!(gnome_answer(true, "No such schema"), None);
        assert_eq!(gnome_answer(false, "false\n"), None);
    }
}
