//! The notification sink, read from the real upstream emulator.

use lumen_harness::{Effect, Origin, Verdict};
use lumen_sinks::Terminal;

fn notifications(t: &Terminal) -> u64 {
    t.log()
        .sinks()
        .iter()
        .find(|s| s.effect == Effect::Notification)
        .map_or(0, |s| s.exercised)
}

#[test]
fn each_notification_form_reaches_the_sink_as_unsealed() {
    for (name, bytes) in [
        ("osc9", &b"\x1b]9;build done\x07"[..]),
        ("osc777", &b"\x1b]777;notify;title;body\x1b\\"[..]),
        ("osc99", &b"\x1b]99;i=1;body text\x1b\\"[..]),
    ] {
        let mut t = Terminal::new();
        t.feed(bytes, Origin::Unsealed, name);
        assert_eq!(notifications(&t), 1, "{name}");
        assert_eq!(t.log().leak_count(), 1, "{name}");
        assert_eq!(t.log().leaks()[0].input, name);
    }
}

#[test]
fn plain_text_and_other_escapes_do_not_reach_the_sink() {
    let mut t = Terminal::new();
    t.feed(
        b"hello\r\n\x1b[31mred\x1b[0m\x1b]0;a title\x07",
        Origin::Unsealed,
        "plain",
    );
    t.feed(b"\x1b]52;c;ZXZpbA==\x07", Origin::Unsealed, "osc52");
    assert_eq!(notifications(&t), 0);
    assert_eq!(t.log().leak_count(), 0);
}

#[test]
fn a_split_notification_is_one_attempt() {
    let mut t = Terminal::new();
    t.feed(b"\x1b]9;half", Origin::Unsealed, "split");
    assert_eq!(notifications(&t), 0);
    t.feed(b" done\x07", Origin::Unsealed, "split");
    assert_eq!(notifications(&t), 1);
}

#[test]
fn the_verdict_is_never_zero_while_other_sinks_have_no_code() {
    let mut t = Terminal::new();
    t.feed(b"nothing to see", Origin::Unsealed, "plain");
    match t.log().verdict() {
        Verdict::NotMeasured(why) => {
            assert!(why.contains("sinks never exercised"), "{why}");
            assert!(why.contains("clipboard"), "{why}");
        }
        other => unreachable!("expected not measured, got {other:?}"),
    }
}
