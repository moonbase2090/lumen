//! The notification sink, read from the real upstream emulator.

use lumen_harness::{Effect, Origin, Verdict};
use lumen_sinks::{Layer, Policy, Terminal};

fn notifications(t: &Terminal) -> u64 {
    t.log()
        .sinks()
        .iter()
        .find(|s| s.effect == Effect::Notification)
        .map_or(0, |s| s.exercised)
}

#[test]
fn each_notification_form_is_denied_before_the_sink_by_default() {
    for (name, bytes) in [
        ("osc9", &b"\x1b]9;build done\x07"[..]),
        ("osc777", &b"\x1b]777;notify;title;body\x1b\\"[..]),
        ("osc99", &b"\x1b]99;i=1;body text\x1b\\"[..]),
        ("osc9-leading-zero", &b"\x1b]009;x\x07"[..]),
    ] {
        let mut t = Terminal::new();
        t.feed(bytes, Origin::Unsealed, name);
        assert_eq!(notifications(&t), 0, "{name}");
        assert_eq!(t.log().leak_count(), 0, "{name}");
        assert_eq!(t.case_sinks().denied_notifications, 1, "{name}");
        assert_eq!(t.case_sinks().notification_attempts, 0, "{name}");
    }
}

fn allowing() -> Policy {
    Policy {
        allow_unsigned: true,
        ..Policy::default()
    }
}

#[test]
fn allow_unsigned_shows_one_toast_a_second_and_never_reaches_the_os_sink() {
    let mut t = Terminal::with_policy(allowing());
    t.feed_at(
        b"\x1b]9;a\x07\x1b]777;notify;t;b\x07\x1b]99;i=1;x\x07",
        Origin::Unsealed,
        "burst",
        0,
    );
    t.feed_at(b"\x1b]9;b\x07", Origin::Unsealed, "soon", 999);
    t.feed_at(b"\x1b]9;c\x07", Origin::Unsealed, "later", 1000);
    assert_eq!(notifications(&t), 0);
    assert_eq!(t.log().leak_count(), 0);
    let c = t.case_sinks();
    assert_eq!(
        (c.toasts, c.denied_notifications, c.notification_attempts),
        (2, 3, 0)
    );
}

#[test]
fn the_opt_in_never_applies_to_ssh_or_a_multiplexer_pane() {
    for layer in [
        Layer {
            ssh: true,
            ..Layer::default()
        },
        Layer {
            multiplexer_pane: true,
            ..Layer::default()
        },
    ] {
        let mut t = Terminal::with_policy(allowing());
        t.set_layer(layer);
        t.feed_at(b"\x1b]9;a\x07", Origin::Unsealed, "x", 0);
        let c = t.case_sinks();
        assert_eq!((c.toasts, c.denied_notifications), (0, 1), "{layer:?}");
        assert_eq!(notifications(&t), 0);
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
fn a_split_notification_is_one_denial() {
    let mut t = Terminal::new();
    t.feed(b"\x1b]9;half", Origin::Unsealed, "split");
    assert_eq!(notifications(&t), 0);
    t.feed(b" done\x07", Origin::Unsealed, "split");
    assert_eq!(notifications(&t), 0);
    assert_eq!(t.case_sinks().denied_notifications, 1);
}

#[test]
fn an_idle_terminal_is_not_measured() {
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
