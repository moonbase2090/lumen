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
            "os_notifications is off: a toast",
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
        "with os_text the program's text follows the fixed prefix",
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

/// A case that mixes unsigned notification sequences with sealed notification
/// frames. `sealed` is what a layer that holds the keys must show and
/// `unsealed` is what an Arm A layer without keys must show.
fn mixed(
    id: &str,
    reads: &[Value],
    context: &Value,
    sealed: &Value,
    unsealed: &Value,
    note: &str,
) -> Value {
    json!({
        "id": id,
        "category": "authority",
        "reads": reads,
        "context": context,
        "sinks": sealed,
        "sinks_unsealed_layer": unsealed,
        "note": note,
    })
}

/// The notification cases of `rt-3` that carry sealed frames, as pretty-printed
/// JSON with a trailing newline. They are the `rt-3` revision: they sit beside
/// the cases of unsigned output and show that a sealed `0x22` frame with the
/// capability is the only way to an OS notification, whatever else is on.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn rt3_sealed_json() -> String {
    const OSC9: &[u8] = b"\x1b]9;x\x07";
    let all_on = json!({"allow_unsigned": true, "os_notifications": true, "os_text": true, "unfocused": true});
    let sealed_os = |extra: &str| -> Value {
        let mut c = all_on.clone();
        if !extra.is_empty() {
            c[extra] = json!(true);
        }
        c
    };
    let mut cases = Vec::new();

    cases.push(mixed(
        "unsigned-never-os-with-every-setting-on",
        &[read(OSC9, 0)],
        &all_on,
        &json!({"effects": {}, "os_notifications": 0, "toasts": 1, "denied_notifications": 0, "notification_attempts": 0}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 1, "denied_notifications": 0, "notification_attempts": 0}),
        "allow_unsigned, os_notifications, os_text and an unfocused window are all on: an unsigned notification is a toast, never an OS notification",
    ));

    let mut f = Frames::new();
    cases.push(mixed(
        "sealed-0x22-reaches-the-os-sink",
        &[read(&f.frame(KIND_GRANT, &grant(8)), 0), read(&f.frame(KIND_NOTIFICATION, &string("build done")), 10)],
        &all_on,
        &json!({"effects": {"grant": 1, "notification": 1}, "os_notifications": 1, "toasts": 0,
                "os_texts": ["Program in pane 1 says: build done"]}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 0, "refused": 0}),
        "a sealed 0x22 frame with the capability, setting on, unfocused, local: the one way to an OS notification",
    ));

    let mut f = Frames::new();
    cases.push(mixed(
        "unsigned-toast-then-sealed-os",
        &[read(OSC9, 0), read(&f.frame(KIND_GRANT, &grant(8)), 5), read(&f.frame(KIND_NOTIFICATION, &string("done")), 1000)],
        &all_on,
        &json!({"effects": {"grant": 1, "notification": 1}, "os_notifications": 1, "toasts": 1}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 1}),
        "the unsigned sequence is a toast and the sealed frame a second later is an OS notification",
    ));

    let mut f = Frames::new();
    cases.push(mixed(
        "sealed-os-then-unsigned-within-a-second",
        &[read(&f.frame(KIND_GRANT, &grant(8)), 0), read(&f.frame(KIND_NOTIFICATION, &string("done")), 10), read(OSC9, 500)],
        &all_on,
        &json!({"effects": {"grant": 1, "notification": 1}, "os_notifications": 1, "toasts": 0, "denied_notifications": 1}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 1, "denied_notifications": 0}),
        "the layer's one notification per second is spent by the sealed frame, so the unsigned sequence 490 ms later is dropped and counted",
    ));

    for (id_, ctx_key, note) in [
        (
            "ssh-sealed-and-unsigned-are-toast-or-nothing",
            "ssh",
            "an SSH session: the sealed frame is a toast, never an OS notification, and the unsigned opt-in does not apply",
        ),
        (
            "multiplexer-sealed-and-unsigned-are-toast-or-nothing",
            "tmux_or_screen",
            "a multiplexer pane: the same",
        ),
    ] {
        let mut f = Frames::new();
        cases.push(mixed(
            id_,
            &[read(&f.frame(KIND_GRANT, &grant(8)), 0), read(&f.frame(KIND_NOTIFICATION, &string("done")), 10), read(OSC9, 1010)],
            &sealed_os(ctx_key),
            &json!({"effects": {"grant": 1}, "os_notifications": 0, "toasts": 1, "denied_notifications": 1}),
            &json!({"effects": {}, "os_notifications": 0, "toasts": 0, "denied_notifications": 1}),
            note,
        ));
    }

    let mut f = Frames::new();
    let mut orphan = all_on.clone();
    orphan["container"] = json!(false);
    cases.push(mixed(
        "orphan-output-unsigned-denied-sealed-os",
        &[read(&f.frame(KIND_GRANT, &grant(8)), 0), read(&f.frame(KIND_NOTIFICATION, &string("done")), 10), read(OSC9, 1010)],
        &orphan,
        &json!({"effects": {"grant": 1, "notification": 1}, "os_notifications": 1, "toasts": 0, "denied_notifications": 1}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 0, "denied_notifications": 1}),
        "orphan output: the opt-in covers grid output only, so the unsigned sequence is denied; the sealed frame is still an OS notification",
    ));

    let mut f = Frames::new();
    let mut focused = all_on.clone();
    focused["unfocused"] = json!(false);
    cases.push(mixed(
        "focused-window-sealed-is-a-toast",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("done")), 10),
        ],
        &focused,
        &json!({"effects": {"grant": 1}, "os_notifications": 0, "toasts": 1}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 0}),
        "a focused window shows a toast instead",
    ));

    let mut f = Frames::new();
    let mut off = all_on.clone();
    off["os_notifications"] = json!(false);
    cases.push(mixed(
        "os-setting-off-sealed-and-unsigned-are-toasts",
        &[
            read(&f.frame(KIND_GRANT, &grant(8)), 0),
            read(&f.frame(KIND_NOTIFICATION, &string("done")), 10),
            read(OSC9, 1010),
        ],
        &off,
        &json!({"effects": {"grant": 1}, "os_notifications": 0, "toasts": 2}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 1}),
        "os_notifications is off: both are toasts",
    ));

    let mut f = Frames::new();
    cases.push(mixed(
        "fifth-toast-in-ten-seconds-is-dropped",
        &[
            read(OSC9, 0),
            read(&f.frame(KIND_GRANT, &grant(8)), 5),
            read(&f.frame(KIND_NOTIFICATION, &string("a")), 1000),
            read(OSC9, 2000),
            read(&f.frame(KIND_NOTIFICATION, &string("b")), 3000),
            read(OSC9, 4000),
        ],
        &off,
        &json!({"effects": {"grant": 1}, "os_notifications": 0, "toasts": 4, "denied_notifications": 1}),
        &json!({"effects": {}, "os_notifications": 0, "toasts": 3, "denied_notifications": 0}),
        "four toasts in ten seconds from a layer, mixing both sources; the fifth is dropped and counted",
    ));

    let mut out = serde_json::to_string_pretty(&json!({
        "revision": "rt-3",
        "note": "notification cases that carry sealed 0x22 frames beside unsigned OSC sequences: an OS notification comes only from a sealed frame with the capability, behind os_notifications; unsigned, SSH, multiplexer and orphan output only ever produce a toast or nothing",
        "cases": cases
    }))
    .unwrap_or_default();
    out.push('\n');
    out
}
