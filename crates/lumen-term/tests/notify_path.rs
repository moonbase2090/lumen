//! The notification path from bytes to the OS: the guard in the document, and
//! sealed frames from the real Arm B layer to the notifier.

use lumen_detector::TAG;
use lumen_sealed::types::{KIND_GRANT, KIND_NOTIFICATION, capability_of};
use lumen_sealed::{Direction, DirectionKeys, HEADER_LEN, LayerId, LayerRoot, Receiver, Sender};
use lumen_term::notify::{Desk, Notifier, Outcome, Place, Recorder, Settings};
use lumen_term::{Context, Document, Event, Layer, Passthrough};

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

fn grid_text(doc: &Document) -> String {
    doc.blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
            lumen_term::Block::Rich(_) => None,
        })
        .collect()
}

#[test]
fn a_notification_sequence_never_reaches_the_grid_and_is_an_event() {
    let mut doc = Document::new(40, 4);
    doc.receive(
        &mut Passthrough,
        b"before\x1b]9;secret words\x07after\r\n",
        at(0),
    );
    let text = grid_text(&doc);
    assert!(text.contains("beforeafter"), "{text:?}");
    assert!(!text.contains("secret"), "{text:?}");
    assert_eq!(
        doc.take_events(),
        [Event::Unsigned {
            number: 9,
            cycle: 0
        }]
    );
    assert_eq!(doc.take_events(), []);
}

#[test]
fn a_sequence_split_across_reads_is_still_removed() {
    let mut doc = Document::new(40, 4);
    for chunk in [
        &b"a\x1b"[..],
        b"]777;notify;t",
        b";body here\x1b",
        b"\\b\r\n",
    ] {
        doc.receive(&mut Passthrough, chunk, at(0));
    }
    let text = grid_text(&doc);
    assert!(text.contains("ab"), "{text:?}");
    assert!(!text.contains("body"), "{text:?}");
    assert_eq!(doc.take_events().len(), 1);
}

#[test]
fn bytes_the_guard_held_back_reach_the_grid_when_the_stream_ends() {
    let mut doc = Document::new(40, 4);
    doc.receive(&mut Passthrough, b"x\x1b]12", at(0));
    doc.finish(&mut Passthrough);
    assert_eq!(doc.take_events(), []);
    assert!(grid_text(&doc).contains('x'));
}

#[test]
fn other_escape_sequences_are_left_alone() {
    let mut doc = Document::new(40, 4);
    doc.receive(
        &mut Passthrough,
        b"\x1b[31mred\x1b[0m \x1b]0;a title\x07ok\r\n",
        at(0),
    );
    assert!(grid_text(&doc).contains("red ok"), "{:?}", grid_text(&doc));
    assert_eq!(doc.take_events(), []);
}

const LAYER: LayerId = LayerId(3);
const ROOT: [u8; 32] = [4; 32];

fn keys() -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new(ROOT), LAYER, Direction::ToTerminal)
}

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

fn frame(sender: &mut Sender, kind: u8, payload: &[u8]) -> Vec<u8> {
    let Ok(sealed) = sender.seal(kind, cap_of(kind), payload) else {
        unreachable!("sealing a short payload succeeds")
    };
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(sealed.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(&sealed);
    assert!(v.len() > HEADER_LEN);
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

fn layer() -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    l.attach_sealed(Receiver::new(keys(), LAYER, Direction::ToTerminal), cap_of);
    Layer::open_container(&mut l);
    l
}

fn local() -> Place {
    Place::Local
}

#[test]
fn sealed_frames_from_the_real_layer_become_an_os_notification_only_under_every_condition() {
    let mut sender = Sender::new(keys(), LAYER, Direction::ToTerminal, 1);
    let grant = frame(&mut sender, KIND_GRANT, &[1, 8]);
    let note = frame(&mut sender, KIND_NOTIFICATION, &string("build done"));
    let mut doc = Document::new(40, 4);
    let mut l = layer();
    doc.receive(&mut l, &grant, at(0));
    doc.receive(&mut l, &note, at(10));
    let events = doc.take_events();
    assert_eq!(events.len(), 2, "{events:?}");

    let settings = Settings {
        os_notifications: true,
        ..Settings::default()
    };
    let mut n = Notifier::new(settings);
    n.set_desk(Desk {
        focused: false,
        muted: false,
    });
    n.add_layer(1, "tab 1", local());
    let mut sink = Recorder::default();
    let out = n.route(1000, 1, &events, &mut sink);
    assert!(matches!(out.as_slice(), [Outcome::Os(_)]), "{out:?}");
    assert_eq!(sink.sent[0].text, "Program in tab 1 says:");

    // The same frames with the setting off are a toast and the sink hears nothing.
    let mut off = Notifier::new(Settings::default());
    off.set_desk(Desk {
        focused: false,
        muted: false,
    });
    off.add_layer(1, "tab 1", local());
    let mut quiet = Recorder::default();
    let out = off.route(1000, 1, &events, &mut quiet);
    assert_eq!(out, [Outcome::Toast]);
    assert_eq!(quiet.sent.len(), 0);
}

#[test]
fn a_forged_sealed_notification_never_becomes_an_event() {
    // Sealed under keys the layer does not hold: the layer refuses it.
    let other = DirectionKeys::derive(&LayerRoot::new([9; 32]), LAYER, Direction::ToTerminal);
    let mut sender = Sender::new(other, LAYER, Direction::ToTerminal, 1);
    let forged = frame(&mut sender, KIND_NOTIFICATION, &string("you won"));
    let mut doc = Document::new(40, 4);
    let mut l = layer();
    doc.receive(&mut l, &forged, at(0));
    assert_eq!(doc.take_events(), []);
}
