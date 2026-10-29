//! The `rt-4` corpus: real sealed frames of the shell-integration, clipboard,
//! notification and marker types, built by the sealing code itself.
//!
//! Every frame is sealed under the fixed test keys the driver gives each
//! replayed layer, with the capability byte the frame type table fixes, so the
//! terminal's sinks are reached by authentic frames. Each case says what the
//! sinks must see. The committed JSON file is the output of [`rt4_json`]; a
//! test fails if the two differ.
//!
//! A clipboard confirmation and a link confirmation are simulated by the
//! harness. The cases that use them say so in `confirm_clipboard` and
//! `user_actions`, and the run report lists the simulated actions.

use crate::{TEST_CAP, TEST_LAYER, TEST_ROOT};
use lumen_detector::TAG;
use lumen_sealed::types::{
    KIND_COMMAND_END, KIND_COMMAND_START, KIND_GRANT, KIND_MARKER, KIND_NOTIFICATION, KIND_PROMPT,
    capability_of,
};
use lumen_sealed::{Direction, DirectionKeys, LayerRoot, Sender};
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The corpus revision.
pub const REVISION: &str = "rt-4";

const CLIPBOARD: u8 = 0x21;

fn sender() -> Sender {
    let keys = DirectionKeys::derive(
        &LayerRoot::new(TEST_ROOT),
        TEST_LAYER,
        Direction::ToTerminal,
    );
    Sender::new(keys, TEST_LAYER, Direction::ToTerminal, 1)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The frames of one case, sealed in order from sequence number 1.
struct Frames(Sender);

impl Frames {
    fn new() -> Self {
        Self(sender())
    }

    /// A frame of `kind` with `payload`, under the capability the table fixes
    /// (or the test capability for a type the table does not have).
    fn frame(&mut self, kind: u8, payload: &[u8]) -> Vec<u8> {
        let cap = capability_of(kind).unwrap_or(TEST_CAP);
        let sealed = match self.0.seal(kind, cap, payload) {
            Ok(s) => s,
            Err(e) => unreachable!("sealing a short payload succeeds: {e:?}"),
        };
        let mut v = b"\x1b_".to_vec();
        v.extend_from_slice(&TAG);
        v.push(kind);
        v.extend_from_slice(&u32::try_from(sealed.len()).unwrap_or(0).to_be_bytes());
        v.extend_from_slice(&sealed);
        v
    }
}

fn id(n: u32) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

fn end(n: u32, status: i32) -> Vec<u8> {
    let mut v = id(n);
    v.extend_from_slice(&status.to_be_bytes());
    v
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn grant(capability: u8) -> Vec<u8> {
    vec![1, capability]
}

fn read(bytes: &[u8], t_ms: u64) -> Value {
    json!({ "hex": hex(bytes), "t_ms": t_ms })
}

/// A case. `sinks` is what a layer that holds the keys must show; a layer
/// without keys (Arm A unsealed) opens nothing, so nothing reaches a sink.
fn case(id: &str, reads: &[Value], context: &Value, sinks: &Value, note: &str) -> Value {
    json!({
        "id": id,
        "category": "authority",
        "reads": reads,
        "context": context,
        "sinks": sinks,
        "sinks_unsealed_layer": {
            "effects": {}, "os_notifications": 0, "refused": 0, "toasts": 0,
            "denied_notifications": 0, "notification_attempts": 0
        },
        "note": note,
    })
}

fn none() -> Value {
    json!({})
}

/// The corpus as pretty-printed JSON with a trailing newline.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn rt4_json() -> String {
    let mut cases = Vec::new();
    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_COMMAND_START, &id(1)), 0),
        read(&f.frame(KIND_PROMPT, &[]), 5),
        read(&f.frame(KIND_COMMAND_END, &end(1, 3)), 10),
    ];
    cases.push(case(
        "cmd-lifecycle",
        &reads,
        &none(),
        &json!({"effects": {"boundary_change": 3, "exit_status": 1}, "refused": 0}),
        "start, prompt and end: three boundary changes and one exit status, all sealed origin",
    ));

    let mut f = Frames::new();
    cases.push(case(
        "cmd-start-out-of-sequence",
        &[read(&f.frame(KIND_COMMAND_START, &id(2)), 0)],
        &none(),
        &json!({"effects": {}, "refused": 1}),
        "the first command id must be 1",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "cmd-end-without-start",
        &[read(&f.frame(KIND_COMMAND_END, &end(1, 0)), 0)],
        &none(),
        &json!({"effects": {}, "refused": 1}),
        "a command end for a command that is not open",
    ));
    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_COMMAND_START, &id(1)), 0),
        read(&f.frame(KIND_COMMAND_END, &end(2, 0)), 5),
    ];
    cases.push(case(
        "cmd-end-wrong-id",
        &reads,
        &none(),
        &json!({"effects": {"boundary_change": 1}, "refused": 1}),
        "the end names another command: refused, and the exit status sink is not reached",
    ));

    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_GRANT, &grant(7)), 0),
        read(&f.frame(KIND_GRANT, &[0, 7]), 5),
        read(&f.frame(CLIPBOARD, b"after the revoke"), 10),
    ];
    cases.push(case(
        "grant-then-revoke",
        &reads,
        &json!({"confirm_clipboard": true}),
        &json!({"effects": {"grant": 2}, "refused": 1}),
        "a revoke takes effect at once, so the clipboard write after it is refused",
    ));
    for (id_, payload, note) in [
        (
            "grant-bad-action",
            vec![2u8, 1],
            "an action other than 0 or 1 is malformed",
        ),
        (
            "grant-capability-nine",
            vec![1, 9],
            "capability numbers stop at 8",
        ),
        (
            "grant-capability-zero",
            vec![1, 0],
            "0 is not a capability that can be granted",
        ),
    ] {
        let mut f = Frames::new();
        cases.push(case(
            id_,
            &[read(&f.frame(KIND_GRANT, &payload), 0)],
            &none(),
            &json!({"effects": {}, "refused": 1}),
            note,
        ));
    }

    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_GRANT, &grant(7)), 0),
        read(&f.frame(CLIPBOARD, b"x"), 5),
    ];
    cases.push(case(
        "grant-denied-by-policy",
        &reads,
        &json!({"deny_grants": [7], "confirm_clipboard": true}),
        &json!({"effects": {}, "refused": 2}),
        "the policy does not allow clipboard-write to be granted: the grant is refused and the write after it too",
    ));
    let mut f = Frames::new();
    let reads = vec![read(&f.frame(KIND_GRANT, &[0, 7]), 0)];
    cases.push(case(
        "revoke-is-never-denied-by-policy",
        &reads,
        &json!({"deny_grants": [7]}),
        &json!({"effects": {"grant": 1}, "refused": 0}),
        "a revoke is allowed whatever the policy denies",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "clipboard-without-grant",
        &[read(&f.frame(CLIPBOARD, b"x"), 0)],
        &json!({"confirm_clipboard": true}),
        &json!({"effects": {}, "refused": 1}),
        "no clipboard-write capability: refused even though the user would confirm",
    ));
    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_GRANT, &grant(7)), 0),
        read(&f.frame(CLIPBOARD, b"copy me"), 5),
    ];
    cases.push(case(
        "clipboard-granted-not-confirmed",
        &reads,
        &none(),
        &json!({"effects": {"grant": 1}, "refused": 0}),
        "granted but the user does not confirm: nothing reaches the clipboard sink",
    ));
    let mut f = Frames::new();
    let reads = vec![
        read(&f.frame(KIND_GRANT, &grant(7)), 0),
        read(&f.frame(CLIPBOARD, b"copy me"), 5),
    ];
    cases.push(case(
        "clipboard-granted-confirmed",
        &reads,
        &json!({"confirm_clipboard": true}),
        &json!({"effects": {"grant": 1, "clipboard_write": 1}, "refused": 0}),
        "granted and confirmed (the confirmation is simulated): one clipboard write, sealed origin",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "notification-without-capability",
        &[read(&f.frame(KIND_NOTIFICATION, &string("hi")), 0)],
        &json!({"os_notifications": true, "unfocused": true}),
        &json!({"effects": {}, "refused": 1, "os_notifications": 0}),
        "no notifications capability: refused",
    ));
    let mut f = Frames::new();
    let two = |f: &mut Frames, gap: u64| {
        vec![
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("build done")), 10),
            read(&f.frame(KIND_NOTIFICATION, &string("again")), 10 + gap),
        ]
    };
    cases.push(case(
        "notification-toast-by-default",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("build done")), 10),
        ],
        &none(),
        &json!({"effects": {"grant": 1}, "toasts": 1, "os_notifications": 0}),
        "OS notifications are off by default: a sealed notification is a toast",
    ));
    for (id_, ctx, note) in [
        (
            "notification-os-but-focused",
            json!({"os_notifications": true}),
            "the terminal is focused: a toast, not an OS notification",
        ),
        (
            "notification-os-but-muted",
            json!({"os_notifications": true, "unfocused": true, "muted": true}),
            "the pane is muted: a toast",
        ),
        (
            "notification-os-but-ssh",
            json!({"os_notifications": true, "unfocused": true, "ssh": true}),
            "an SSH session never raises an OS notification",
        ),
        (
            "notification-os-but-multiplexer",
            json!({"os_notifications": true, "unfocused": true, "tmux_or_screen": true}),
            "a multiplexer pane never raises an OS notification",
        ),
        (
            "notification-os-setting-off",
            json!({"unfocused": true}),
            "notifications.os is off: a toast",
        ),
    ] {
        let mut f = Frames::new();
        cases.push(case(
            id_,
            &[
                read(&f.frame(KIND_GRANT, &grant(8)), 0),
                read(&f.frame(KIND_NOTIFICATION, &string("build done")), 10),
            ],
            &ctx,
            &json!({"effects": {"grant": 1}, "toasts": 1, "os_notifications": 0}),
            note,
        ));
    }
    let mut f = Frames::new();
    cases.push(case(
        "notification-os-unfocused",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("secret build output")), 10),
        ],
        &json!({"os_notifications": true, "unfocused": true}),
        &json!({
            "effects": {"grant": 1, "notification": 1}, "os_notifications": 1, "toasts": 0,
            "os_texts": ["Program in pane 1 says:"]
        }),
        "setting on, capability held, unfocused, local, not muted: exactly one OS notification, labelled, with no program text",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "notification-os-text-opt-in",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("build done")), 10),
        ],
        &json!({"os_notifications": true, "os_text": true, "unfocused": true}),
        &json!({
            "effects": {"grant": 1, "notification": 1}, "os_notifications": 1,
            "os_texts": ["Program in pane 1 says: build done"]
        }),
        "with notifications.os-text the program's text follows the fixed prefix",
    ));
    let mut f = Frames::new();
    let long = "x".repeat(300);
    cases.push(case(
        "notification-os-text-capped",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(
                &f.frame(
                    KIND_NOTIFICATION,
                    &string(&format!("a\u{202e}b\u{200b}c {long}")),
                ),
                10,
            ),
        ],
        &json!({"os_notifications": true, "os_text": true, "unfocused": true}),
        &json!({"effects": {"grant": 1, "notification": 1}, "os_notifications": 1}),
        "bidirectional and zero-width characters are removed and the text is capped",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "notification-rate-limit-500ms",
        &two(&mut f, 500),
        &json!({"os_notifications": true, "unfocused": true}),
        &json!({
            "effects": {"grant": 1, "notification": 1}, "os_notifications": 1,
            "denied_notifications": 1
        }),
        "a second notification 500 ms after the first is dropped and counted",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "notification-rate-limit-1000ms",
        &two(&mut f, 1000),
        &json!({"os_notifications": true, "unfocused": true}),
        &json!({
            "effects": {"grant": 1, "notification": 2}, "os_notifications": 2,
            "denied_notifications": 0
        }),
        "1000 ms apart both are accepted",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "notification-malformed-length",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &[0, 9, b'h', b'i']), 10),
        ],
        &none(),
        &json!({"effects": {"grant": 1}, "refused": 1, "toasts": 0}),
        "the length field disagrees with the text",
    ));

    let mut f = Frames::new();
    let mut marker = id(3);
    marker.extend_from_slice(&[7; 32]);
    cases.push(case(
        "marker-has-no-effect",
        &[read(&f.frame(KIND_MARKER, &marker), 0)],
        &none(),
        &json!({"effects": {}, "refused": 0}),
        "a staged-content marker is parsed and has no other effect in this revision",
    ));
    let mut f = Frames::new();
    cases.push(case(
        "unknown-type",
        &[read(&f.frame(0x05, &[1, 2, 3]), 0)],
        &none(),
        &json!({"effects": {}, "refused": 1}),
        "a type that is not in the table is refused",
    ));

    for (id_, action, effects, note) in [
        (
            "link-open-confirmed",
            "open_link_confirmed",
            json!({"link_open": 1}),
            "the user confirms the full target (simulated): one link open, user origin, not a leak",
        ),
        (
            "link-open-not-confirmed",
            "open_link_unconfirmed",
            json!({}),
            "no confirmation: the link sink is not reached",
        ),
    ] {
        let mut c = case(
            id_,
            &[read(b"ls\n", 0)],
            &none(),
            &json!({"effects": effects.clone()}),
            note,
        );
        c["user_actions"] = json!([action]);
        c["sinks_unsealed_layer"] = json!({"effects": effects});
        cases.push(c);
    }

    let mut out = serde_json::to_string_pretty(&json!({
        "revision": REVISION,
        "note": "sealed frames of the shell-integration, clipboard, notification and marker types, sealed under the fixed test keys; every case says what the sinks must see",
        "cases": cases
    }))
    .unwrap_or_default();
    out.push('\n');
    out
}
