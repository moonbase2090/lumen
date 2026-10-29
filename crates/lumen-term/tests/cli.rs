//! The command line, and a snapshot taken through the real binary.

use lumen_term::cli::Options;
use std::time::Duration;

fn parse(args: &[&str]) -> Result<Options, String> {
    Options::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
}

#[test]
fn no_arguments_is_the_default_shell_in_a_default_window() {
    let o = parse(&[]).unwrap_or_default();
    assert_eq!(o, Options::default());
    assert_eq!(o.size, (800, 600));
    assert!(o.command.is_empty() && o.snapshot.is_none() && o.scale.is_none());
}

#[test]
fn flags_and_a_command() {
    let o = parse(&[
        "--scale",
        "2",
        "--size",
        "640x480",
        "--snapshot",
        "out.png",
        "--wait",
        "250",
        "--exit-after",
        "900",
        "--",
        "sh",
        "-c",
        "echo hi",
    ])
    .unwrap_or_default();
    assert_eq!(o.scale, Some(2));
    assert_eq!(o.size, (640, 480));
    assert_eq!(o.snapshot.as_deref(), Some("out.png"));
    assert_eq!(o.wait, Duration::from_millis(250));
    assert_eq!(o.exit_after, Some(Duration::from_millis(900)));
    assert_eq!(o.command, ["sh", "-c", "echo hi"]);
}

#[test]
fn the_first_word_that_is_not_a_flag_starts_the_command() {
    let o = parse(&["--scale", "1", "top", "-d", "1"]).unwrap_or_default();
    assert_eq!(o.command, ["top", "-d", "1"]);
}

#[test]
fn bad_arguments_are_refused() {
    for bad in [
        &["--scale"][..],
        &["--scale", "0"],
        &["--scale", "5"],
        &["--scale", "x"],
        &["--size", "640"],
        &["--size", "0x480"],
        &["--size", "axb"],
        &["--snapshot"],
        &["--wait", "soon"],
        &["--frobnicate"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_binary_writes_a_snapshot_of_a_programs_output() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("snapshot.png");
    let _ = std::fs::remove_file(&path);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
        .args(["--size", "320x190", "--wait", "5000", "--snapshot"])
        .arg(&path)
        .args(["--", "/bin/sh", "-c", "echo shown"])
        .output();
    let Ok(out) = out else {
        unreachable!("the binary runs")
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("320x190 ->"));
    let file = std::fs::File::open(&path).unwrap_or_else(|e| unreachable!("snapshot exists: {e}"));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap_or_else(|e| unreachable!("png reads: {e}"));
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .unwrap_or_else(|e| unreachable!("png frame: {e}"));
    assert_eq!((info.width, info.height), (320, 190));
    let (pixels, _) = buf[..info.buffer_size()].as_chunks::<4>();
    let ink = pixels.iter().filter(|p| p[..3] != [255, 255, 255]).count();
    assert!(ink > 100, "the program's text is drawn: {ink} inked pixels");
}

#[test]
fn the_binary_refuses_bad_arguments_with_the_usage() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
        .arg("--frobnicate")
        .output();
    let Ok(out) = out else {
        unreachable!("the binary runs")
    };
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: lumen-term"));
}

#[test]
fn notification_flags_are_off_by_default_and_parse() {
    let d = parse(&[]).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!d.says_local);
    let s = d.notifications;
    assert_eq!(
        (s.allow_unsigned, s.os_notifications, s.os_text),
        (false, false, false)
    );
    let o = parse(&[
        "--allow-unsigned",
        "--os-notifications",
        "--os-text",
        "--local",
        "--",
        "x",
    ])
    .unwrap_or_else(|e| unreachable!("{e}"));
    let s = o.notifications;
    assert_eq!(
        (s.allow_unsigned, s.os_notifications, s.os_text),
        (true, true, true)
    );
    assert!(o.says_local);
    assert_eq!(o.command, ["x"]);
    assert!(
        parse(&["--os-text"]).is_err(),
        "the text needs the setting it belongs to"
    );
}

#[cfg(unix)]
fn snapshot_bytes(flags: &[&str], name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_file(&path);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
        .env_remove("TMUX")
        .env_remove("STY")
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .env_remove("SSH_CLIENT")
        .args(["--size", "480x300", "--wait", "5000", "--snapshot"])
        .arg(&path)
        .args(flags)
        .args(["--", "/bin/sh", "-c", "printf '\\033]9;hello\\007'"])
        .output();
    let Ok(out) = out else {
        unreachable!("the binary runs")
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read(&path).unwrap_or_else(|e| unreachable!("snapshot exists: {e}"))
}

#[cfg(unix)]
#[test]
fn the_real_binary_shows_a_toast_only_when_the_flags_allow_it() {
    let plain = snapshot_bytes(&[], "toast-off.png");
    let not_stated = snapshot_bytes(&["--allow-unsigned"], "toast-unknown.png");
    let shown = snapshot_bytes(&["--allow-unsigned", "--local"], "toast-on.png");
    assert_eq!(
        plain, not_stated,
        "an unknown place is not local, so no toast"
    );
    assert_ne!(
        plain, shown,
        "with the setting and a stated local place the toast is drawn"
    );
    let without_setting = snapshot_bytes(&["--local"], "toast-no-setting.png");
    assert_eq!(
        plain, without_setting,
        "stating the place does not turn the setting on"
    );
}
