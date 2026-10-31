//! Gate G13, no authority from unsealed input: no unsealed input creates command
//! structure or exercises any capability ([`gate.no-authority`]). The one
//! exception is the user's `allow_unsigned` opt-in, which lets unsigned grid
//! output on a local layer draw a terminal toast, at most one per layer per
//! second.
//!
//! Every input here is written by a program with no key. The layer under test
//! holds a real receiver key, so the only thing between the bytes and authority
//! is that they are not sealed. Authority has three observable forms in the
//! terminal: a sealed frame that opens ([`Event::Sealed`] and the document's
//! sealed list), a notification the notifier acts on, and a change to the layer's
//! grants. A control run with validly sealed frames shows each is observable.

use lumen_detector::TAG;
use lumen_sealed::types::{KIND_GRANT, KIND_NOTIFICATION, capability_of};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver, Sender};
use lumen_term::notify::{Desk, Dropped, Notifier, Outcome, Place, Recorder, Settings, Source};
use lumen_term::{Block, Context, Document, Event, Layer, Notice, Piece};

const LAYER: LayerId = LayerId(3);
const ID: u32 = 1;

fn keys() -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new([4; 32]), LAYER, Direction::ToTerminal)
}

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

fn layer() -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    l.attach_sealed(Receiver::new(keys(), LAYER, Direction::ToTerminal), cap_of);
    Layer::open_container(&mut l);
    l
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// A frame sealed under the layer's real key.
fn sealed(sender: &mut Sender, kind: u8, payload: &[u8]) -> Vec<u8> {
    let Ok(body) = sender.seal(kind, cap_of(kind), payload) else {
        unreachable!("sealing a short payload succeeds")
    };
    frame(kind, &body)
}

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

/// Inputs aimed at command structure, the clipboard, the title, notifications,
/// grants or a reply, written by a program that holds no key.
fn unsealed_inputs() -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = Vec::new();
    // Every frame type byte, with the payloads the sealed types expect.
    let payloads: [(&str, Vec<u8>); 5] = [
        ("empty", Vec::new()),
        ("grant-notifications", vec![1, 8]),
        ("command-start", vec![0, 0, 0, 1]),
        ("text", string("rm -rf ~")),
        ("exit-0", vec![0, 0, 0, 0]),
    ];
    for kind in 0..=255u8 {
        for (label, p) in &payloads {
            v.push((format!("frame-{kind:#04x}-{label}"), frame(kind, p)));
        }
    }
    let osc = |n: &str, body: &str| format!("\x1b]{n};{body}\x07").into_bytes();
    let named: Vec<(&str, Vec<u8>)> = vec![
        ("osc52-write", osc("52", "c;c2VjcmV0")),
        ("osc52-query", osc("52", "c;?")),
        ("osc0-title", osc("0", "verified exit 0")),
        ("osc2-title", osc("2", "title")),
        ("osc7-cwd", osc("7", "file://evil/etc")),
        (
            "osc8-link",
            b"\x1b]8;;https://evil.example\x07click\x1b]8;;\x07".to_vec(),
        ),
        ("osc9", osc("9", "you won")),
        ("osc99", osc("99", "i=1:d=0;hello")),
        ("osc133-prompt", osc("133", "A")),
        ("osc133-command-end", osc("133", "D;0")),
        ("osc777", osc("777", "notify;title;body")),
        ("osc1337", osc("1337", "File=inline=1:AAAA")),
        ("osc-c1", [&[0x9d][..], b"52;c;c2VjcmV0\x07"].concat()),
        ("osc-st-terminated", b"\x1b]9;x\x1b\\".to_vec()),
        ("dcs-xtgettcap", b"\x1bP+q544e\x1b\\".to_vec()),
        ("dcs-sixel", b"\x1bPq#0;2;0;0;0~-\x1b\\".to_vec()),
        (
            "apc-kitty-graphics",
            b"\x1b_Gf=24,s=1,v=1;AAAA\x1b\\".to_vec(),
        ),
        ("pm", b"\x1b^private\x1b\\".to_vec()),
        ("sos", b"\x1bXstring\x1b\\".to_vec()),
        ("da1-query", b"\x1b[c".to_vec()),
        ("da2-query", b"\x1b[>c".to_vec()),
        ("dsr-query", b"\x1b[6n".to_vec()),
        ("decrqm-query", b"\x1b[?2004$p".to_vec()),
        ("xtversion-query", b"\x1b[>0q".to_vec()),
        ("title-report", b"\x1b[21t".to_vec()),
        ("paste-mode", b"\x1b[?2004h".to_vec()),
        ("mouse-mode", b"\x1b[?1003h\x1b[?1006h".to_vec()),
        ("answerback", b"\x05".to_vec()),
        ("bell", b"\x07\x07\x07".to_vec()),
        ("full-reset", b"\x1bc".to_vec()),
    ];
    for (name, bytes) in named {
        v.push((name.to_string(), bytes));
    }
    // A grant then a notification, both in the sealed range, neither sealed.
    let mut forged = frame(KIND_GRANT, &[1, 8]);
    forged.extend(frame(KIND_NOTIFICATION, &string("granted by me")));
    v.push(("forged-grant-then-notification".into(), forged));
    v
}

/// Feed `bytes` to a fresh document and layer, whole or one byte at a time.
fn receive(bytes: &[u8], single: bool) -> (Document, lumen_arm_b::Layer) {
    let mut d = Document::new(60, 8);
    let mut l = layer();
    if single {
        for b in bytes {
            d.receive(&mut l, std::slice::from_ref(b), at(0));
        }
        d.finish(&mut l);
    } else {
        d.receive(&mut l, bytes, at(0));
    }
    (d, l)
}

fn notifier(settings: Settings, place: Place) -> Notifier {
    let mut n = Notifier::new(settings);
    n.set_desk(Desk {
        focused: false,
        muted: false,
    });
    assert!(n.add_layer(ID, "tab 1", place));
    n
}

const EVERYTHING_ON: Settings = Settings {
    allow_unsigned: true,
    os_notifications: true,
    os_text: true,
};

#[test]
fn the_control_run_shows_authority_is_observable_when_the_input_is_sealed() {
    let mut sender = Sender::new(keys(), LAYER, Direction::ToTerminal, 1);
    let mut bytes = sealed(&mut sender, KIND_GRANT, &[1, 8]);
    bytes.extend(sealed(
        &mut sender,
        KIND_NOTIFICATION,
        &string("build done"),
    ));
    let (mut d, _) = receive(&bytes, false);
    assert_eq!(d.sealed().len(), 2, "both sealed frames opened");
    let events = d.take_events();
    assert!(events.iter().all(|e| matches!(e, Event::Sealed { .. })));
    let mut n = notifier(Settings::default(), Place::Local);
    let mut sink = Recorder::default();
    let out = n.route(1000, ID, &events, &mut sink);
    assert_eq!(out, [Outcome::Toast], "a granted sealed notification shows");
    assert!(n.granted(ID), "the sealed grant changed the layer's grants");
}

#[test]
fn no_unsealed_input_opens_a_sealed_frame_or_changes_a_grant() {
    let mut checked = 0;
    for (name, bytes) in unsealed_inputs() {
        for single in [false, true] {
            let (mut d, mut l) = receive(&bytes, single);
            assert_eq!(d.sealed(), [], "{name} single={single}: a frame opened");
            let events = d.take_events();
            for e in &events {
                match e {
                    Event::Unsigned { number, .. } => assert!(
                        matches!(number, 9 | 99 | 777),
                        "{name}: unsigned event for OSC {number}"
                    ),
                    Event::StringTruncated { .. } => {}
                    Event::Sealed { kind, .. } => {
                        unreachable!("{name} single={single}: sealed event {kind:#04x}")
                    }
                }
            }
            // The events change no grant and cause no notification of any kind.
            let mut n = notifier(Settings::default(), Place::Local);
            let mut sink = Recorder::default();
            n.route(1000, ID, &events, &mut sink);
            assert!(!n.granted(ID), "{name}: a grant was made");
            assert_eq!(n.shown(), [], "{name}: a toast was shown by default");
            assert_eq!(sink.sent, [], "{name}: the OS was notified");
            let rest = Layer::finish(&mut l);
            assert!(
                rest.iter().all(|p| !matches!(p, Piece::Sealed { .. })),
                "{name}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, (256 * 5 + 30 + 1) * 2);
}

#[test]
fn unsealed_input_makes_only_grid_and_track_0_blocks_and_no_command_structure() {
    for (name, bytes) in unsealed_inputs() {
        let (d, _) = receive(&bytes, false);
        for block in d.blocks() {
            match block {
                Block::Grid(_) | Block::Rich(_) => {}
            }
        }
        assert!(
            d.notices()
                .iter()
                .all(|n| matches!(n, Notice::Degraded | Notice::Pinned)),
            "{name}: unexpected notice"
        );
        assert_eq!(d.sealed(), [], "{name}");
    }
    // Command structure is the sealed prompt, command start and command end. A
    // run of them unsealed opens no sealed frame, so nothing marks a command.
    let mut structure = Vec::new();
    for kind in [0x10, 0x11, 0x12] {
        structure.extend(frame(kind, &[0, 0, 0, 0]));
    }
    let (mut d, _) = receive(&structure, false);
    assert_eq!(d.sealed(), []);
    assert_eq!(d.take_events(), []);
}

#[test]
fn unsealed_output_is_denied_by_default_and_every_attempt_is_counted() {
    let mut n = notifier(Settings::default(), Place::Local);
    let mut sink = Recorder::default();
    let mut attempts = 0u64;
    for (name, bytes) in unsealed_inputs() {
        let (mut d, _) = receive(&bytes, false);
        let events = d.take_events();
        let unsigned = events
            .iter()
            .filter(|e| matches!(e, Event::Unsigned { .. }))
            .count() as u64;
        let out = n.route(10_000 + attempts, ID, &events, &mut sink);
        assert!(
            out.iter().all(|o| *o == Outcome::Dropped(Dropped::Denied)),
            "{name}: {out:?}"
        );
        attempts += unsigned;
    }
    // OSC 9, 99, 777 and the ST-terminated OSC 9 of the corpus.
    assert_eq!(attempts, 4, "the corpus has four notification sequences");
    assert_eq!(n.dropped(), attempts, "every attempt is counted");
    assert_eq!(n.shown(), []);
    assert_eq!(sink.sent, []);
}

#[test]
fn allow_unsigned_is_the_only_exception_a_local_toast_never_an_os_notification() {
    let mut n = notifier(EVERYTHING_ON, Place::Local);
    let mut sink = Recorder::default();
    let (mut d, _) = receive(
        b"\x1b]9;hello\x07\x1b]777;notify;t;b\x07\x1b]99;i=1;x\x07",
        false,
    );
    let events = d.take_events();
    assert_eq!(events.len(), 3);
    let out = n.route(5000, ID, &events, &mut sink);
    assert_eq!(out[0], Outcome::Toast);
    assert!(
        n.shown()[0].unsigned,
        "the toast carries the unverified label"
    );
    assert!(
        out[1..]
            .iter()
            .all(|o| *o == Outcome::Dropped(Dropped::Spacing)),
        "at most one per second: {out:?}"
    );
    assert_eq!(sink.sent, [], "unsigned output never reaches the OS");
}

#[test]
fn at_most_one_unsigned_toast_per_layer_per_second_over_a_minute() {
    let mut n = notifier(EVERYTHING_ON, Place::Local);
    let mut shown_at = Vec::new();
    // A sequence every 37 ms for a minute, so many arrive inside each second.
    let mut t = 0;
    while t < 60_000 {
        if n.notify(t, ID, Source::UnsignedGrid, "") == Outcome::Toast {
            shown_at.push(t);
        }
        t += 37;
    }
    // The ceiling is one a second; other limits (toasts per window) may lower
    // the count, so only a floor of "some were shown" keeps this from being vacuous.
    assert!(shown_at.len() >= 2, "toasts were shown: {}", shown_at.len());
    assert!(shown_at.len() <= 60, "{} toasts in 60 s", shown_at.len());
    for pair in shown_at.windows(2) {
        assert!(pair[1] - pair[0] >= 1000, "toasts at {pair:?}");
    }
}

#[test]
fn allow_unsigned_never_applies_to_a_layer_that_is_not_shown_local() {
    for place in [
        Place::Ssh,
        Place::MultiplexerPane,
        Place::PossiblyRemote,
        Place::Unknown,
    ] {
        let mut n = notifier(EVERYTHING_ON, place);
        for (name, bytes) in unsealed_inputs() {
            let (mut d, _) = receive(&bytes, false);
            let mut sink = Recorder::default();
            n.route(100_000, ID, &d.take_events(), &mut sink);
            assert_eq!(n.shown(), [], "{name}: toast on {place:?}");
            assert_eq!(sink.sent, [], "{name}: OS notification on {place:?}");
        }
    }
}
