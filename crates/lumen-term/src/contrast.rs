//! Whether the system asks for more contrast.
//!
//! macOS has "Increase contrast" in Accessibility, a GNOME desktop has its
//! `high-contrast` setting, and Windows has a High Contrast theme. When the
//! system says so, a terminal that follows the system draws with the built-in
//! high-contrast palette; a user who fixed a palette keeps it. The answer comes
//! from a fixed system program started with fixed arguments (never a shell),
//! and anything unexpected (a missing program, an error, unrecognised text)
//! means no answer, and the look stays as it was.

use std::process::{Command, Stdio};

/// The preference behind macOS's Increase contrast: `1` when it is on. The key
/// is absent until it has been turned on once, which makes `defaults` fail.
const MACOS_READ: (&str, [&str; 3]) = (
    "/usr/bin/defaults",
    ["read", "com.apple.universalaccess", "increaseContrast"],
);

/// The GNOME setting: `true` while high contrast is on.
const GNOME_READ: (&str, [&str; 3]) = (
    "/usr/bin/gsettings",
    ["get", "org.gnome.desktop.a11y.interface", "high-contrast"],
);

/// What `defaults` says when the key has never been written.
const MACOS_ABSENT: &str =
    "The domain/default pair of (com.apple.universalaccess, increaseContrast) does not exist";

/// What `defaults read com.apple.universalaccess increaseContrast` said: `1` is
/// on and `0` is off. The key is absent until the setting has been turned on
/// once, and then `defaults` fails saying that very pair does not exist: that
/// is off. Any other failure (including the same words about another domain or
/// key), or text that is not `0` or `1`, is no answer.
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

/// What `gsettings get ... high-contrast` said: `true` is on and `false` is
/// off. Anything else is no answer.
#[must_use]
pub fn gnome_answer(succeeded: bool, stdout: &str) -> Option<bool> {
    if !succeeded {
        return None;
    }
    match stdout.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// What `reg query "HKCU\Control Panel\Accessibility\HighContrast" /v Flags`
/// said. The value is a string of flags and the lowest bit (`HCF_HIGHCONTRASTON`)
/// is set while a High Contrast theme is on. Output that has no `Flags` line
/// with a number is no answer.
#[must_use]
pub fn windows_answer(succeeded: bool, stdout: &str) -> Option<bool> {
    if !succeeded {
        return None;
    }
    stdout.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next()? == "Flags").then_some(())?;
        words.next().filter(|kind| kind.starts_with("REG_"))?;
        let flags: u64 = words.next()?.parse().ok()?;
        Some(flags & 1 == 1)
    })
}

fn run(program: &str, args: &[&str]) -> Option<(bool, String, String)> {
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

/// Whether the system is set to increase contrast right now, or `None` when it
/// cannot be told. A caller keeps what it last knew on `None`.
#[must_use]
pub fn system_increases_contrast() -> Option<bool> {
    match std::env::consts::OS {
        "macos" => {
            let (ok, out, err) = run(MACOS_READ.0, &MACOS_READ.1)?;
            macos_answer(ok, &out, &err)
        }
        "linux" => {
            let (ok, out, _) = run(GNOME_READ.0, &GNOME_READ.1)?;
            gnome_answer(ok, &out)
        }
        "windows" => {
            // The system folder is named by the system; the program is the one in it.
            let root = std::env::var_os("SystemRoot")?;
            let reg = std::path::Path::new(&root).join("System32").join("reg.exe");
            let (ok, out, _) = run(
                reg.to_str()?,
                &[
                    "query",
                    r"HKCU\Control Panel\Accessibility\HighContrast",
                    "/v",
                    "Flags",
                ],
            )?;
            windows_answer(ok, &out)
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
        let absent = "The domain/default pair of (com.apple.universalaccess, increaseContrast) does not exist\n";
        assert_eq!(macos_answer(false, "", absent), Some(false));
    }

    #[test]
    fn a_macos_read_that_failed_another_way_or_said_something_else_is_no_answer() {
        assert_eq!(macos_answer(false, "", "Operation not permitted"), None);
        let other =
            "The domain/default pair of (com.apple.universalaccess, reduceMotion) does not exist\n";
        assert_eq!(macos_answer(false, "", other), None);
        assert_eq!(macos_answer(false, "", ""), None);
        assert_eq!(macos_answer(true, "", ""), None);
        assert_eq!(macos_answer(true, "2\n", ""), None);
    }

    #[test]
    fn gnome_true_is_on_false_is_off_and_the_rest_is_no_answer() {
        assert_eq!(gnome_answer(true, "true\n"), Some(true));
        assert_eq!(gnome_answer(true, "false\n"), Some(false));
        assert_eq!(gnome_answer(true, ""), None);
        assert_eq!(gnome_answer(true, "No such schema"), None);
        assert_eq!(gnome_answer(false, "true\n"), None);
    }

    #[test]
    fn windows_reads_the_low_bit_of_the_flags_value() {
        let on = "\r\nHKEY_CURRENT_USER\\Control Panel\\Accessibility\\HighContrast\r\n    Flags    REG_SZ    127\r\n\r\n";
        let off = "\r\nHKEY_CURRENT_USER\\Control Panel\\Accessibility\\HighContrast\r\n    Flags    REG_SZ    126\r\n\r\n";
        assert_eq!(windows_answer(true, on), Some(true));
        assert_eq!(windows_answer(true, off), Some(false));
    }

    #[test]
    fn windows_output_that_is_not_a_flags_number_is_no_answer() {
        assert_eq!(windows_answer(false, "    Flags    REG_SZ    127"), None);
        assert_eq!(windows_answer(true, ""), None);
        assert_eq!(windows_answer(true, "    Flags    REG_SZ    on"), None);
        assert_eq!(windows_answer(true, "    Other    REG_SZ    127"), None);
        assert_eq!(
            windows_answer(
                true,
                "ERROR: The system was unable to find the specified registry key or value."
            ),
            None
        );
    }
}
