//! The notification rules: toasts, OS notifications, budgets and text.

use lumen_term::Event;
use lumen_term::notify::{
    Desk, Dropped, MARKER, MAX_TEXT_BYTES, MAX_TEXT_CELLS, Notifier, Outcome, Place, Recorder,
    Settings, Source, clean, compose, escape_markup,
};

const LOCAL: Place = Place::Local;

fn all_on() -> Settings {
    Settings {
        allow_unsigned: true,
        os_notifications: true,
        os_text: false,
    }
}

fn unfocused() -> Desk {
    Desk {
        focused: false,
        muted: false,
    }
}

fn notifier(settings: Settings, place: Place) -> Notifier {
    let mut n = Notifier::new(settings);
    n.set_desk(unfocused());
    assert!(n.add_layer(1, "tab 1", place));
    n
}

fn sealed(n: &mut Notifier, now: u64, text: &str) -> Outcome {
    n.notify(now, 1, Source::Sealed, text)
}

#[test]
fn everything_is_off_by_default_and_unsigned_output_is_denied_and_counted() {
    let mut n = notifier(Settings::default(), LOCAL);
    assert_eq!(
        n.notify(0, 1, Source::UnsignedGrid, ""),
        Outcome::Dropped(Dropped::Denied)
    );
    assert_eq!(n.dropped(), 1);
    assert_eq!(n.shown().len(), 0);
}

#[test]
fn allow_unsigned_draws_a_toast_on_a_local_layer_and_never_an_os_notification() {
    let mut n = notifier(all_on(), LOCAL);
    assert_eq!(n.notify(0, 1, Source::UnsignedGrid, ""), Outcome::Toast);
    assert!(n.shown()[0].unsigned);
    assert!(n.shown()[0].text.starts_with("Program in tab 1 says:"));
}

#[test]
fn allow_unsigned_never_applies_to_a_layer_that_is_not_shown_local() {
    for place in [
        Place::Unknown,
        Place::Ssh,
        Place::MultiplexerPane,
        Place::PossiblyRemote,
    ] {
        let mut n = notifier(all_on(), place);
        assert_eq!(
            n.notify(0, 1, Source::UnsignedGrid, ""),
            Outcome::Dropped(Dropped::Denied),
            "{place:?}"
        );
    }
}

#[test]
fn a_sealed_notification_is_an_os_notification_only_when_every_condition_holds() {
    let mut n = notifier(all_on(), LOCAL);
    let Outcome::Os(note) = sealed(&mut n, 0, "build done") else {
        unreachable!("every condition holds")
    };
    assert_eq!(
        note.text, "Program in tab 1 says:",
        "no program text by default"
    );

    let cases: [(&str, Settings, Place, Desk); 5] = [
        (
            "setting off",
            Settings {
                os_notifications: false,
                ..all_on()
            },
            LOCAL,
            unfocused(),
        ),
        (
            "focused",
            all_on(),
            LOCAL,
            Desk {
                focused: true,
                muted: false,
            },
        ),
        ("not local", all_on(), Place::Unknown, unfocused()),
        ("ssh", all_on(), Place::Ssh, unfocused()),
        ("multiplexer", all_on(), Place::MultiplexerPane, unfocused()),
    ];
    for (name, settings, place, desk) in cases {
        let mut n = Notifier::new(settings);
        n.set_desk(desk);
        n.add_layer(1, "tab 1", place);
        assert_eq!(
            sealed(&mut n, 0, "x"),
            Outcome::Toast,
            "{name}: a toast instead"
        );
    }
}

#[test]
fn program_text_reaches_the_os_only_with_os_text_and_is_cleaned_capped_and_escaped() {
    let mut n = notifier(
        Settings {
            os_text: true,
            ..all_on()
        },
        LOCAL,
    );
    let Outcome::Os(note) = sealed(&mut n, 0, "<b>a & b</b>\u{202e}x") else {
        unreachable!("every condition holds")
    };
    assert_eq!(
        note.text,
        "Program in tab 1 says: &lt;b&gt;a &amp; b&lt;/b&gt;\u{27e8}U+202E\u{27e9}x"
    );
    let mut n = notifier(all_on(), LOCAL);
    let Outcome::Os(plain) = sealed(&mut n, 0, "secret") else {
        unreachable!("every condition holds")
    };
    assert!(!plain.text.contains("secret"));
}

#[test]
fn tokens_are_single_use_per_notification_and_bound_to_a_layer() {
    let mut n = Notifier::new(all_on());
    n.set_desk(unfocused());
    n.add_layer(1, "tab 1", LOCAL);
    n.add_layer(2, "tab 2", LOCAL);
    let (Outcome::Os(a), Outcome::Os(b)) =
        (sealed(&mut n, 0, "x"), n.notify(0, 2, Source::Sealed, "y"))
    else {
        unreachable!("both are OS notifications")
    };
    assert_ne!(a.token, b.token);
    assert_eq!((a.layer, b.layer), (1, 2));
}

#[test]
fn the_per_layer_os_budget_is_three_a_minute_then_toasts_until_the_user_acts() {
    let mut n = notifier(all_on(), LOCAL);
    for t in [0, 1000, 2000] {
        assert!(matches!(sealed(&mut n, t, "x"), Outcome::Os(_)), "at {t}");
    }
    assert_eq!(
        sealed(&mut n, 3000, "x"),
        Outcome::Toast,
        "the fourth is a toast"
    );
    // The user opens the layer: the block lifts, but the window still holds three.
    n.user_opened(1);
    assert_eq!(
        sealed(&mut n, 4000, "x"),
        Outcome::Toast,
        "still three in the last minute"
    );
    // The window slides past the first two, but the layer stays toasts only
    // until the user acts on it or ten quiet minutes pass.
    assert_eq!(sealed(&mut n, 61_000, "x"), Outcome::Toast);
    n.user_opened(1);
    assert!(matches!(sealed(&mut n, 62_000, "x"), Outcome::Os(_)));
}

#[test]
fn ten_quiet_minutes_also_end_a_spent_budget() {
    let mut n = notifier(all_on(), LOCAL);
    for t in [0, 1000, 2000] {
        sealed(&mut n, t, "x");
    }
    assert_eq!(sealed(&mut n, 3000, "x"), Outcome::Toast);
    // Not yet ten minutes since the last notification from the layer.
    assert_eq!(sealed(&mut n, 3000 + 599_000, "x"), Outcome::Toast);
    // Ten minutes with nothing from it, and the window is empty.
    assert!(matches!(
        sealed(&mut n, 3000 + 599_000 + 600_000, "x"),
        Outcome::Os(_)
    ));
}

#[test]
fn the_global_os_budget_is_eight_a_minute_and_a_noisy_layer_cannot_take_anothers_toasts() {
    let mut n = Notifier::new(all_on());
    n.set_desk(unfocused());
    for id in 1..=4 {
        n.add_layer(id, &format!("tab {id}"), LOCAL);
    }
    let mut os = 0;
    for (i, id) in [1, 1, 1, 2, 2, 2, 3, 3, 3, 4].into_iter().enumerate() {
        let t = u64::try_from(i).unwrap_or(0) * 1000;
        if matches!(n.notify(t, id, Source::Sealed, "x"), Outcome::Os(_)) {
            os += 1;
        }
    }
    assert_eq!(os, 8, "no more than eight in a minute across layers");
    // Layer 4 has had no toast of its own taken.
    let Outcome::Toast = n.notify(20_000, 4, Source::Sealed, "again") else {
        unreachable!("layer 4 still gets its toast")
    };
}

#[test]
fn toast_rate_limits() {
    let mut n = notifier(Settings::default(), LOCAL);
    assert_eq!(sealed(&mut n, 0, "a"), Outcome::Toast);
    assert_eq!(sealed(&mut n, 500, "b"), Outcome::Dropped(Dropped::Spacing));
    for t in [1000, 2000, 3000] {
        assert_eq!(sealed(&mut n, t, "c"), Outcome::Toast, "at {t}");
    }
    assert_eq!(
        sealed(&mut n, 4000, "d"),
        Outcome::Dropped(Dropped::ToastLimit),
        "fifth in ten seconds"
    );
    assert_eq!(n.dropped(), 2);
    // Toasts last six seconds.
    n.tick(5999);
    assert_eq!(n.shown().len(), 4);
    n.tick(9001);
    assert_eq!(n.shown().len(), 0);
    // The window moves on.
    assert_eq!(sealed(&mut n, 10_000, "e"), Outcome::Toast);
}

#[test]
fn a_full_screen_replaces_the_layers_own_oldest_toast_and_never_another_layers() {
    let mut n = Notifier::new(Settings::default());
    for id in 1..=5 {
        n.add_layer(id, &format!("tab {id}"), LOCAL);
    }
    // Four layers fill sixteen toasts within six seconds, each within its own limits.
    for round in 0..4u64 {
        for id in 1..=4 {
            assert_eq!(
                n.notify(round * 1000, id, Source::Sealed, &format!("r{round}")),
                Outcome::Toast
            );
        }
    }
    assert_eq!(n.shown().len(), 16);
    // Layer 5 has none of its own to replace: its toast is dropped.
    assert_eq!(
        n.notify(3000, 5, Source::Sealed, "x"),
        Outcome::Dropped(Dropped::NoRoom)
    );
    // Expire layer 1's toasts only by closing it, then it has room again.
    let others = n.shown().iter().filter(|x| x.layer != 1).count();
    n.close_layer(1);
    assert_eq!(n.shown().iter().filter(|x| x.layer != 1).count(), others);
    assert!(
        n.shown().iter().all(|x| x.layer != 1),
        "a closed layer's toasts go with it"
    );
}

#[test]
fn muting_silences_toasts_and_unmuting_is_the_user_acting() {
    let mut n = notifier(all_on(), LOCAL);
    n.set_muted(1, true);
    assert_eq!(sealed(&mut n, 0, "x"), Outcome::Dropped(Dropped::Muted));
    n.set_muted(1, false);
    assert!(matches!(sealed(&mut n, 1000, "x"), Outcome::Os(_)));
    let mut n = notifier(all_on(), LOCAL);
    n.set_desk(Desk {
        focused: false,
        muted: true,
    });
    assert_eq!(
        sealed(&mut n, 0, "x"),
        Outcome::Dropped(Dropped::Muted),
        "the terminal-wide mute"
    );
}

#[test]
fn layer_ids_are_never_reused() {
    let mut n = Notifier::new(all_on());
    assert!(n.add_layer(7, "tab 1", LOCAL));
    n.close_layer(7);
    assert!(!n.add_layer(7, "tab 9", LOCAL));
    assert_eq!(
        n.notify(0, 7, Source::Sealed, "x"),
        Outcome::Dropped(Dropped::NoLayer)
    );
}

#[test]
fn clean_removes_what_is_not_text_and_shows_what_draws_nothing_as_an_escape() {
    // Controls, line and paragraph separators, private use and unassigned
    // code points are removed; every default-ignorable and bidi code point is
    // shown as a visible escape, so nothing in a notification is invisible.
    let nasty = "a\u{0}\u{7}b\u{2028}e\u{2029}f\u{e000}h\u{378}i";
    assert_eq!(clean(nasty), "abefhi");
    // Two short strings: every escape is nine bytes, and the cap still applies.
    assert_eq!(
        clean("a\u{202e}b\u{200b}c\u{e0041}d\u{ad}e"),
        "a\u{27e8}U+202E\u{27e9}b\u{27e8}U+200B\u{27e9}c\u{27e8}U+E0041\u{27e9}d\u{27e8}U+00AD\u{27e9}e"
    );
    assert_eq!(
        clean("\u{115f}\u{1160}\u{3164}\u{ffa0}f"),
        "\u{27e8}U+115F\u{27e9}\u{27e8}U+1160\u{27e9}\u{27e8}U+3164\u{27e9}\u{27e8}U+FFA0\u{27e9}f"
    );
    assert_eq!(
        clean("\u{2062}g\u{feff}h\u{200d}i\u{fe0f}j"),
        "\u{27e8}U+2062\u{27e9}g\u{27e8}U+FEFF\u{27e9}h\u{27e8}U+200D\u{27e9}i\u{27e8}U+FE0F\u{27e9}j"
    );
    assert_eq!(
        clean("tab\there\nnow"),
        "tabherenow",
        "a control character is removed, not turned into a space"
    );
}

#[test]
fn clean_keeps_one_combining_mark_per_base_and_drops_a_mark_with_no_base() {
    assert_eq!(clean("e\u{301}"), "e\u{301}");
    assert_eq!(clean("e\u{301}\u{302}\u{303}"), "e\u{301}");
    assert_eq!(clean("\u{301}a"), "a", "no base before the mark");
    assert_eq!(
        clean("a\u{301}b\u{302}"),
        "a\u{301}b\u{302}",
        "one per base"
    );
}

#[test]
fn clean_caps_bytes_and_cells_and_ends_a_cut_text_with_the_marker() {
    let long = clean(&"x".repeat(500));
    assert!(
        long.len() <= MAX_TEXT_BYTES && long.ends_with(MARKER),
        "{}",
        long.len()
    );
    // Wide characters: sixty cells is thirty of them, fewer once the marker is counted.
    let wide = clean(&"\u{4e2d}".repeat(100));
    let cells: usize = wide.chars().map(|c| if c == MARKER { 1 } else { 2 }).sum();
    assert!(cells <= MAX_TEXT_CELLS, "{cells}");
    assert!(wide.ends_with(MARKER) && wide.len() <= MAX_TEXT_BYTES);
    // Within both caps nothing is cut.
    assert_eq!(clean("short"), "short");
    let exact = "y".repeat(MAX_TEXT_CELLS);
    assert_eq!(clean(&exact), exact);
}

#[test]
fn compose_has_the_fixed_prefix_and_the_label_the_program_cannot_remove() {
    assert_eq!(
        compose("tab 2 \u{25cf}", None),
        "Program in tab 2 \u{25cf} says:"
    );
    assert_eq!(
        compose("tab 2", Some("\u{202e}hi")),
        "Program in tab 2 says: \u{27e8}U+202E\u{27e9}hi"
    );
    assert_eq!(
        escape_markup("<a href='x'>&\"</a>"),
        "&lt;a href=&#39;x&#39;&gt;&amp;&quot;&lt;/a&gt;"
    );
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn sealed_event(kind: u8, body: Vec<u8>) -> Event {
    Event::Sealed { kind, body }
}

#[test]
fn route_needs_the_grant_then_sends_os_notifications_to_the_sink() {
    let mut n = notifier(all_on(), LOCAL);
    let mut sink = Recorder::default();
    // No grant: a sealed notification is dropped, counted, and goes nowhere.
    let first = n.route(0, 1, &[sealed_event(0x22, string("hi"))], &mut sink);
    assert_eq!(first, [Outcome::Dropped(Dropped::Denied)]);
    // Grant notifications (capability 8), then notify.
    let out = n.route(
        1000,
        1,
        &[
            sealed_event(0x20, vec![1, 8]),
            sealed_event(0x22, string("done")),
        ],
        &mut sink,
    );
    assert!(matches!(out.as_slice(), [Outcome::Os(_)]));
    assert_eq!(sink.sent.len(), 1);
    // Revoke: back to dropped.
    n.revoke(1);
    let out = n.route(5000, 1, &[sealed_event(0x22, string("again"))], &mut sink);
    assert_eq!(out, [Outcome::Dropped(Dropped::Denied)]);
    assert_eq!(sink.sent.len(), 1);
}

#[test]
fn route_ignores_other_capabilities_malformed_frames_and_other_types() {
    let mut n = notifier(all_on(), LOCAL);
    let mut sink = Recorder::default();
    let out = n.route(
        0,
        1,
        &[
            sealed_event(0x20, vec![1, 7]), // clipboard-write, not notifications
            sealed_event(0x20, vec![9, 8]), // malformed grant
            sealed_event(0x22, vec![0, 5, 1]), // malformed notification
            sealed_event(0x12, vec![0; 8]), // a command end, not ours
        ],
        &mut sink,
    );
    assert_eq!(out, []);
    assert!(!n.granted(1));
    assert_eq!(sink.sent.len(), 0);
}

#[test]
fn route_turns_an_unsigned_sequence_into_a_toast_only_with_the_opt_in() {
    let mut sink = Recorder::default();
    let event = Event::Unsigned {
        number: 9,
        cycle: 0,
    };
    let mut off = notifier(Settings::default(), LOCAL);
    assert_eq!(
        off.route(0, 1, std::slice::from_ref(&event), &mut sink),
        [Outcome::Dropped(Dropped::Denied)]
    );
    let mut on = notifier(all_on(), LOCAL);
    assert_eq!(on.route(0, 1, &[event], &mut sink), [Outcome::Toast]);
    assert_eq!(sink.sent.len(), 0, "never the OS");
}

#[test]
fn a_layer_at_the_screen_cap_replaces_only_its_own_oldest_toast() {
    let mut n = Notifier::new(Settings::default());
    for id in 1..=6 {
        n.add_layer(id, &format!("tab {id}"), LOCAL);
    }
    // Layers 1 to 5 show three toasts each and layer 6 one: sixteen in all.
    for round in 0..3u64 {
        for id in 1..=5 {
            assert_eq!(
                n.notify(round * 1000, id, Source::Sealed, &format!("l{id}r{round}")),
                Outcome::Toast
            );
        }
    }
    assert_eq!(n.notify(3000, 6, Source::Sealed, "six"), Outcome::Toast);
    assert_eq!(n.shown().len(), 16);
    let before: Vec<_> = n.shown().iter().filter(|t| t.layer != 1).cloned().collect();
    // Layer 1 has one more toast allowed in its window, and the screen is full.
    assert_eq!(n.notify(3500, 1, Source::Sealed, "new"), Outcome::Toast);
    assert_eq!(n.shown().len(), 16, "one in, one out");
    let after: Vec<_> = n.shown().iter().filter(|t| t.layer != 1).cloned().collect();
    assert_eq!(before, after, "no other layer lost a toast");
    let mine: Vec<_> = n
        .shown()
        .iter()
        .filter(|t| t.layer == 1)
        .map(|t| t.text.clone())
        .collect();
    assert_eq!(mine.len(), 3);
    assert!(mine.last().is_some_and(|t| t.ends_with("new")), "{mine:?}");
    assert!(
        !mine.iter().any(|t| t.ends_with("l1r0")),
        "its own oldest went: {mine:?}"
    );
}

#[test]
fn a_host_that_never_reports_focus_gets_toasts_not_os_notifications() {
    // No call to set_desk: the terminal assumes it has focus.
    let mut n = Notifier::new(all_on());
    n.add_layer(1, "tab 1", LOCAL);
    assert!(Desk::default().focused && !Desk::default().muted);
    for t in [0, 1000, 2000] {
        assert_eq!(sealed(&mut n, t, "x"), Outcome::Toast, "at {t}");
    }
    // Once the host says the terminal is unfocused, the OS path opens.
    n.set_desk(unfocused());
    assert!(matches!(sealed(&mut n, 3000, "x"), Outcome::Os(_)));
    // Back to focused, and it is a toast again.
    n.set_desk(Desk {
        focused: true,
        muted: false,
    });
    assert_eq!(sealed(&mut n, 4000, "x"), Outcome::Toast);
}

#[test]
fn routing_without_a_desk_report_never_reaches_the_os_sink() {
    let mut n = Notifier::new(all_on());
    n.add_layer(1, "tab 1", LOCAL);
    let mut sink = Recorder::default();
    let events = [
        sealed_event(0x20, vec![1, 8]),
        sealed_event(0x22, string("hi")),
    ];
    let out = n.route(0, 1, &events, &mut sink);
    assert_eq!(out, [Outcome::Toast]);
    assert_eq!(sink.sent.len(), 0);
}
