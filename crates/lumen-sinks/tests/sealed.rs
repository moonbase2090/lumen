//! Sealed frames routed to the sinks, and the user's link action.

use lumen_harness::{Origin, Verdict};
use lumen_sealed::types::{
    KIND_COMMAND_END, KIND_COMMAND_START, KIND_GRANT, KIND_NOTIFICATION, KIND_PROMPT, Malformed,
};
use lumen_sinks::sealed::{Outcome, Refusal};
use lumen_sinks::{Desk, Layer, Policy, Terminal};

fn start(id: u32) -> Vec<u8> {
    id.to_be_bytes().to_vec()
}

fn end(id: u32, status: i32) -> Vec<u8> {
    let mut v = id.to_be_bytes().to_vec();
    v.extend_from_slice(&status.to_be_bytes());
    v
}

fn note(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn count(t: &Terminal, effect: &str) -> u64 {
    t.case_sinks().effects.get(effect).copied().unwrap_or(0)
}

fn granted(capability: u8) -> Vec<u8> {
    vec![1, capability]
}

#[test]
fn a_command_lifecycle_reaches_the_boundary_and_exit_sinks_as_sealed() {
    let mut t = Terminal::new();
    assert_eq!(
        t.sealed(KIND_COMMAND_START, &start(1), 0, "c"),
        Outcome::Applied
    );
    assert_eq!(t.sealed(KIND_PROMPT, &[], 0, "c"), Outcome::Applied);
    assert_eq!(
        t.sealed(KIND_COMMAND_END, &end(1, 3), 0, "c"),
        Outcome::Applied
    );
    assert_eq!(count(&t, "boundary_change"), 3);
    assert_eq!(count(&t, "exit_status"), 1);
    assert_eq!(t.log().leak_count(), 0);
}

#[test]
fn out_of_sequence_commands_are_refused_and_reach_no_sink() {
    let mut t = Terminal::new();
    let bad = Outcome::Refused(Refusal::OutOfSequence);
    assert_eq!(t.sealed(KIND_COMMAND_START, &start(2), 0, "c"), bad);
    assert_eq!(t.sealed(KIND_COMMAND_END, &end(1, 0), 0, "c"), bad);
    assert_eq!(
        t.sealed(KIND_COMMAND_START, &start(1), 0, "c"),
        Outcome::Applied
    );
    assert_eq!(t.sealed(KIND_COMMAND_END, &end(9, 0), 0, "c"), bad);
    assert_eq!(count(&t, "exit_status"), 0);
    assert_eq!(count(&t, "boundary_change"), 1);
    assert_eq!(t.case_sinks().refused, 3);
}

#[test]
fn unknown_and_malformed_frames_are_refused() {
    let mut t = Terminal::new();
    assert_eq!(
        t.sealed(0x05, &[], 0, "c"),
        Outcome::Refused(Refusal::UnknownKind)
    );
    assert_eq!(
        t.sealed(KIND_GRANT, &[2, 1], 0, "c"),
        Outcome::Refused(Refusal::Malformed(Malformed::Field))
    );
    assert_eq!(
        t.sealed(KIND_PROMPT, &[1], 0, "c"),
        Outcome::Refused(Refusal::Malformed(Malformed::Length))
    );
    assert_eq!(t.case_sinks().refused, 3);
    assert!(t.case_sinks().effects.is_empty());
}

#[test]
fn a_clipboard_write_needs_the_grant_and_the_users_confirmation() {
    let mut t = Terminal::new();
    assert_eq!(
        t.sealed(0x21, b"x", 0, "c"),
        Outcome::Refused(Refusal::NotGranted)
    );
    assert_eq!(t.sealed(KIND_GRANT, &granted(7), 0, "c"), Outcome::Applied);
    assert_eq!(t.sealed(0x21, b"x", 0, "c"), Outcome::Unconfirmed);
    assert_eq!(count(&t, "clipboard_write"), 0);
    assert_eq!(t.simulated(), [] as [&str; 0]);
    t.set_desk(Desk {
        confirm_clipboard: true,
        ..Desk::default()
    });
    assert_eq!(t.sealed(0x21, b"x", 0, "c"), Outcome::Applied);
    assert_eq!(count(&t, "clipboard_write"), 1);
    assert_eq!(t.simulated(), ["clipboard_confirmation"]);
    // A revoke takes effect at once.
    assert_eq!(t.sealed(KIND_GRANT, &[0, 7], 0, "c"), Outcome::Applied);
    assert_eq!(
        t.sealed(0x21, b"x", 0, "c"),
        Outcome::Refused(Refusal::NotGranted)
    );
}

#[test]
fn a_notification_needs_the_capability() {
    let mut t = Terminal::new();
    assert_eq!(
        t.sealed(KIND_NOTIFICATION, &note("hi"), 0, "c"),
        Outcome::Refused(Refusal::NotGranted)
    );
}

fn os_terminal(text: bool) -> Terminal {
    let mut t = Terminal::with_policy(Policy {
        os_notifications: true,
        os_text: text,
        ..Policy::default()
    });
    t.set_desk(Desk {
        unfocused: true,
        ..Desk::default()
    });
    assert_eq!(t.sealed(KIND_GRANT, &granted(8), 0, "c"), Outcome::Applied);
    t
}

#[test]
fn an_os_notification_needs_every_condition_and_carries_no_program_text_by_default() {
    let mut t = os_terminal(false);
    assert_eq!(
        t.sealed(KIND_NOTIFICATION, &note("secret"), 0, "c"),
        Outcome::Applied
    );
    assert_eq!(t.case_sinks().os_notifications, 1);
    assert_eq!(t.case_sinks().os_texts, ["Program in pane 1 says:"]);
    assert_eq!(count(&t, "notification"), 1);
    assert_eq!(t.log().leak_count(), 0);

    for (name, desk, layer, policy) in [
        ("focused", Desk::default(), Layer::default(), true),
        (
            "muted",
            Desk {
                unfocused: true,
                muted: true,
                ..Desk::default()
            },
            Layer::default(),
            true,
        ),
        (
            "ssh",
            Desk {
                unfocused: true,
                ..Desk::default()
            },
            Layer {
                ssh: true,
                ..Layer::default()
            },
            true,
        ),
        (
            "multiplexer",
            Desk {
                unfocused: true,
                ..Desk::default()
            },
            Layer {
                multiplexer_pane: true,
                ..Layer::default()
            },
            true,
        ),
        (
            "setting off",
            Desk {
                unfocused: true,
                ..Desk::default()
            },
            Layer::default(),
            false,
        ),
    ] {
        let mut t = Terminal::with_policy(Policy {
            os_notifications: policy,
            ..Policy::default()
        });
        t.set_desk(desk);
        t.set_layer(layer);
        t.sealed(KIND_GRANT, &granted(8), 0, "c");
        t.sealed(KIND_NOTIFICATION, &note("x"), 0, "c");
        assert_eq!(t.case_sinks().os_notifications, 0, "{name}");
        assert_eq!(t.case_sinks().toasts, 1, "{name}: shown as a toast instead");
        assert_eq!(count(&t, "notification"), 0, "{name}");
    }
}

#[test]
fn the_program_text_is_added_only_with_its_own_setting_and_is_cleaned_and_capped() {
    let mut t = os_terminal(true);
    let text = format!("a\u{1b}[31mb\u{202e}c\u{200b}d {}", "x".repeat(300));
    t.sealed(KIND_NOTIFICATION, &note(&text), 0, "c");
    let shown = &t.case_sinks().os_texts[0];
    assert!(
        shown.starts_with("Program in pane 1 says: a[31mbcd xxx"),
        "{shown}"
    );
    assert!(shown.ends_with('\u{2026}'), "{shown}");
    assert!(
        !shown
            .chars()
            .any(|c| c.is_control() || c == '\u{202e}' || c == '\u{200b}')
    );
    let program_part = shown.trim_start_matches("Program in pane 1 says: ");
    assert!(
        program_part.len() <= 200 + '\u{2026}'.len_utf8(),
        "{}",
        program_part.len()
    );
    assert!(program_part.chars().count() <= 61);
}

#[test]
fn notifications_are_rate_limited_per_layer() {
    let mut t = os_terminal(false);
    for (at, os) in [(0, 1), (500, 1), (1000, 2), (1999, 2), (2000, 3)] {
        t.sealed(KIND_NOTIFICATION, &note("x"), at, "c");
        assert_eq!(t.case_sinks().os_notifications, os, "at {at}");
    }
    assert_eq!(t.case_sinks().denied_notifications, 2);
}

#[test]
fn toasts_stop_at_four_in_ten_seconds() {
    let mut t = Terminal::new();
    t.sealed(KIND_GRANT, &granted(8), 0, "c");
    for i in 0..6u64 {
        t.sealed(KIND_NOTIFICATION, &note("x"), i * 1000, "c");
    }
    assert_eq!(t.case_sinks().toasts, 4);
    assert_eq!(t.case_sinks().denied_notifications, 2);
    t.sealed(KIND_NOTIFICATION, &note("x"), 10_000, "c");
    assert_eq!(t.case_sinks().toasts, 5, "the window has moved on");
}

#[test]
fn a_link_opens_only_after_the_users_confirmation_and_is_not_a_leak() {
    let mut t = Terminal::new();
    t.user_opens_link(false, "c");
    assert_eq!(count(&t, "link_open"), 0);
    t.user_opens_link(true, "c");
    assert_eq!(count(&t, "link_open"), 1);
    assert_eq!(t.log().leak_count(), 0);
    assert_eq!(t.simulated(), ["link_confirmation"]);
}

#[test]
fn the_verdict_is_zero_only_when_all_six_sinks_were_exercised_and_none_leaked() {
    let mut t = os_terminal(false);
    t.set_desk(Desk {
        unfocused: true,
        confirm_clipboard: true,
        ..Desk::default()
    });
    assert!(matches!(t.log().verdict(), Verdict::NotMeasured(_)));
    t.sealed(KIND_COMMAND_START, &start(1), 0, "c");
    t.sealed(KIND_COMMAND_END, &end(1, 0), 0, "c");
    t.sealed(KIND_GRANT, &granted(7), 0, "c");
    t.sealed(0x21, b"copy", 0, "c");
    t.sealed(KIND_NOTIFICATION, &note("x"), 0, "c");
    // Five of six: no link yet.
    match t.log().verdict() {
        Verdict::NotMeasured(why) => assert!(why.contains("link_open"), "{why}"),
        other => unreachable!("expected not measured, got {other:?}"),
    }
    t.user_opens_link(true, "c");
    assert_eq!(t.log().verdict(), Verdict::Zero);
    // One unsigned notification that got past the guard would make it non-zero.
    t.feed(b"plain", Origin::Unsealed, "c");
    assert_eq!(t.log().verdict(), Verdict::Zero);
}

#[test]
fn negative_control_a_missing_guard_makes_the_count_non_zero() {
    let mut t = Terminal::new();
    t.disable_guard_for_negative_control();
    t.feed(b"\x1b]9;build done\x07", Origin::Unsealed, "osc9");
    assert_eq!(
        t.log().leak_count(),
        1,
        "the tripwire sees what the guard would have removed"
    );
    assert_eq!(t.log().verdict(), Verdict::NonZero(1));
}

#[test]
fn a_grant_the_policy_does_not_allow_is_refused_and_a_revoke_is_not() {
    let mut t = Terminal::with_policy(Policy {
        grant_denied: 1 << 7,
        ..Policy::default()
    });
    assert_eq!(
        t.sealed(KIND_GRANT, &granted(7), 0, "c"),
        Outcome::Refused(Refusal::PolicyDenied)
    );
    assert_eq!(t.sealed(KIND_GRANT, &granted(8), 0, "c"), Outcome::Applied);
    assert_eq!(t.sealed(KIND_GRANT, &[0, 7], 0, "c"), Outcome::Applied);
    assert_eq!(count(&t, "grant"), 2);
    assert_eq!(t.case_sinks().refused, 1);
}
