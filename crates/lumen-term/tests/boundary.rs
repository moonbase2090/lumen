//! The sealed boundary frames that decide whether the input box may take keys
//! on a sealed layer: only a prompt with no command start after it counts, a
//! frame out of order makes the frames say nothing, and only frames that were
//! sealed under the layer's key are seen at all.

use lumen_detector::TAG;
use lumen_sealed::types::{KIND_COMMAND_END, KIND_COMMAND_START, KIND_PROMPT, capability_of};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver, Sender};
use lumen_term::boundary::{Boundaries, Seen};
use lumen_term::{Context, Document, Event};

fn start(id: u32) -> (u8, Vec<u8>) {
    (KIND_COMMAND_START, id.to_be_bytes().to_vec())
}

fn end(id: u32, status: i32) -> (u8, Vec<u8>) {
    let mut body = id.to_be_bytes().to_vec();
    body.extend_from_slice(&status.to_be_bytes());
    (KIND_COMMAND_END, body)
}

fn prompt() -> (u8, Vec<u8>) {
    (KIND_PROMPT, Vec::new())
}

fn after(frames: &[(u8, Vec<u8>)]) -> Boundaries {
    let mut b = Boundaries::new();
    for (kind, body) in frames {
        b.see(*kind, body);
    }
    b
}

#[test]
fn nothing_seen_yet_is_not_a_prompt() {
    assert_eq!(Boundaries::new().at_prompt(), Some(false));
}

#[test]
fn a_prompt_is_a_prompt_until_a_command_starts() {
    assert_eq!(after(&[prompt()]).at_prompt(), Some(true));
    assert_eq!(after(&[prompt(), start(1)]).at_prompt(), Some(false));
    assert_eq!(
        after(&[prompt(), start(1), end(1, 0)]).at_prompt(),
        Some(false)
    );
    assert_eq!(
        after(&[prompt(), start(1), end(1, 0), prompt()]).at_prompt(),
        Some(true)
    );
    assert_eq!(
        after(&[prompt(), start(1), end(1, 1), prompt(), start(2)]).at_prompt(),
        Some(false)
    );
}

#[test]
fn a_command_start_out_of_sequence_is_refused_and_the_frames_say_nothing() {
    let mut b = after(&[prompt(), start(1), end(1, 0), prompt()]);
    let (kind, body) = start(5);
    assert_eq!(b.see(kind, &body), Seen::Refused);
    assert_eq!(b.at_prompt(), None);
    // Nothing after a refusal brings the frames back.
    let (kind, body) = prompt();
    assert_eq!(b.see(kind, &body), Seen::Refused);
    assert_eq!(b.at_prompt(), None);
}

#[test]
fn a_command_end_with_no_matching_start_is_refused() {
    let mut b = after(&[prompt(), start(1)]);
    let (kind, body) = end(2, 0);
    assert_eq!(b.see(kind, &body), Seen::Refused);
    assert_eq!(b.at_prompt(), None);
}

#[test]
fn a_malformed_boundary_is_refused() {
    let mut b = after(&[prompt()]);
    assert_eq!(b.see(KIND_PROMPT, &[0]), Seen::Refused);
    assert_eq!(b.at_prompt(), None);
}

#[test]
fn frames_that_are_not_boundaries_change_nothing() {
    let mut b = after(&[prompt()]);
    for kind in [0x01, 0x02, 0x20, 0x21, 0x22, 0x30, 0x7f] {
        assert_eq!(b.see(kind, &[1, 2, 3]), Seen::Other);
    }
    assert_eq!(b.at_prompt(), Some(true));
}

const LAYER: LayerId = LayerId(1);

fn keys() -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new([9; 32]), LAYER, Direction::ToTerminal)
}

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

/// An APC with the seal tag around `body`, as it travels on the PTY.
fn apc(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// What a document fed `bytes`, behind a layer holding a real key, makes of
/// the boundary frames in them.
fn through_a_layer(bytes: &[u8]) -> Boundaries {
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    layer.attach_sealed(Receiver::new(keys(), LAYER, Direction::ToTerminal), cap_of);
    let mut d = Document::new(60, 8);
    d.receive(&mut layer, bytes, Context::default());
    let mut b = Boundaries::new();
    for event in d.take_events() {
        if let Event::Sealed { kind, body, .. } = event {
            b.see(kind, &body);
        }
    }
    b
}

#[test]
fn a_sealed_prompt_from_the_layer_counts() {
    let mut sender = Sender::new(keys(), LAYER, Direction::ToTerminal, 1);
    let Ok(body) = sender.seal(KIND_PROMPT, cap_of(KIND_PROMPT), &[]) else {
        unreachable!("sealing an empty payload succeeds")
    };
    assert_eq!(
        through_a_layer(&apc(KIND_PROMPT, &body)).at_prompt(),
        Some(true)
    );
}

#[test]
fn a_prompt_frame_that_does_not_open_is_never_seen() {
    // A program with no key writes the prompt frame in the clear, and then a
    // body sealed under another layer's key: neither opens.
    let mut other = Sender::new(
        DirectionKeys::derive(&LayerRoot::new([7; 32]), LAYER, Direction::ToTerminal),
        LAYER,
        Direction::ToTerminal,
        1,
    );
    let Ok(foreign) = other.seal(KIND_PROMPT, cap_of(KIND_PROMPT), &[]) else {
        unreachable!("sealing an empty payload succeeds")
    };
    let mut bytes = apc(KIND_PROMPT, &[]);
    bytes.extend(apc(KIND_PROMPT, &foreign));
    assert_eq!(through_a_layer(&bytes).at_prompt(), Some(false));
}

#[test]
fn after_a_forwarded_enter_the_box_waits_for_the_shells_next_frame() {
    let mut b = after(&[prompt()]);
    assert!(b.between_commands());
    b.line_sent();
    assert!(b.between_commands(), "nothing has read the line yet");
    assert_eq!(
        b.at_prompt(),
        Some(false),
        "the old prompt no longer counts"
    );
    // An empty line: the shell draws its prompt again with no command.
    let (kind, body) = prompt();
    b.see(kind, &body);
    assert_eq!(b.at_prompt(), Some(true));
    // A command: its start ends the wait, and only the next prompt counts.
    b.line_sent();
    let (kind, body) = start(1);
    b.see(kind, &body);
    assert!(
        !b.between_commands(),
        "a command may be reading now, not the next line"
    );
    assert_eq!(b.at_prompt(), Some(false));
    let (kind, body) = end(1, 0);
    b.see(kind, &body);
    assert!(b.between_commands(), "the command ended");
    assert_eq!(b.at_prompt(), Some(false), "the prompt has not come yet");
    let (kind, body) = prompt();
    b.see(kind, &body);
    assert_eq!(b.at_prompt(), Some(true));
}

#[test]
fn after_a_refusal_nothing_is_awaited() {
    let mut b = after(&[prompt()]);
    b.line_sent();
    let (kind, body) = start(9);
    assert_eq!(b.see(kind, &body), Seen::Refused);
    assert!(!b.between_commands());
    assert_eq!(b.at_prompt(), None);
}
