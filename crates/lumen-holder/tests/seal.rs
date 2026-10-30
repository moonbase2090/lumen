//! Seal requests without a process: what the holder seals for the shell, what
//! it refuses, and that what it seals is what the terminal's receiver opens.

use lumen_holder::layer::{Layer, Outcome};
use lumen_holder::principal::{Creds, Principal, Terminal};
use lumen_holder::protocol::{
    self, MAX_MESSAGE, MAX_SEAL_BODY, SEAL_REFUSED, SEALED, SEALED_HEADER_LEN, SealRefusal, VERSION,
};
use lumen_sealed::types::{
    KIND_CLIPBOARD_WRITE, KIND_COMMAND_END, KIND_COMMAND_START, KIND_GRANT, KIND_LAYOUT,
    KIND_MARKER, KIND_NOTIFICATION, KIND_PANE_CONTAINER, KIND_PROMPT,
};
use lumen_sealed::{
    Direction, DirectionKeys, HeaderError, LayerId, LayerRoot, PayloadError, Receiver,
};

const TERMINAL: Terminal = Terminal {
    pid: 100,
    uid: 1000,
};
const FROM_TERMINAL: Option<Creds> = Some(Creds {
    pid: 100,
    uid: 1000,
    gid: 1000,
});
const SHELL: Principal = Principal {
    pid: 4242,
    start_time: 99_000,
    uid: 1000,
};
const FROM_SHELL: Option<Creds> = Some(Creds {
    pid: 4242,
    uid: 1000,
    gid: 1000,
});
const FROM_CHILD: Option<Creds> = Some(Creds {
    pid: 4300,
    uid: 1000,
    gid: 1000,
});
const ROOT: [u8; 32] = [0x5a; 32];
const LAYER: u32 = 12;

fn clock(t: u64) -> impl Fn(u32) -> Option<u64> {
    move |_| Some(t)
}

/// A layer that is ready and holds its key.
fn live() -> Layer {
    let mut layer = Layer::new(SHELL, TERMINAL);
    layer.handle(FROM_SHELL, &protocol::ready(1), false, &clock(99_000));
    let key = protocol::key_message(1, LAYER, &ROOT);
    layer.handle_spawn(FROM_TERMINAL, &key[..], false, &|_| true);
    assert!(layer.is_ready() && layer.has_key());
    layer
}

fn ask(layer: &mut Layer, number: u32, kind: u8, body: &[u8]) -> Outcome {
    let m = protocol::seal_request(number, kind, body)
        .unwrap_or_else(|| unreachable!("the test's payloads fit a request"));
    layer.handle(FROM_SHELL, &m, false, &clock(99_000))
}

fn receiver() -> Receiver {
    Receiver::new(
        DirectionKeys::derive(&LayerRoot::new(ROOT), LayerId(LAYER), Direction::ToTerminal),
        LayerId(LAYER),
        Direction::ToTerminal,
    )
}

/// The sealed frame inside a `sealed` answer, for the request `number`.
fn frame_of(reply: &[u8], number: u32) -> Vec<u8> {
    assert_eq!(&reply[..2], &[VERSION, SEALED]);
    assert_eq!(reply[2..SEALED_HEADER_LEN], number.to_be_bytes());
    reply[SEALED_HEADER_LEN..].to_vec()
}

fn refusal_of(reply: &[u8], number: u32) -> u8 {
    assert_eq!(&reply[..2], &[VERSION, SEAL_REFUSED]);
    assert_eq!(reply[2..6], number.to_be_bytes());
    assert_eq!(reply.len(), protocol::SEAL_REFUSED_LEN);
    reply[6]
}

#[test]
fn a_command_start_is_sealed_and_the_terminals_receiver_opens_it() {
    let mut layer = live();
    assert_eq!(
        ask(&mut layer, 7, KIND_COMMAND_START, &1u32.to_be_bytes()),
        Outcome::Sealed
    );
    let reply = layer.take_reply().unwrap_or_default();
    let (header, plain) = receiver()
        .open(0, &frame_of(&reply, 7))
        .unwrap_or_else(|e| unreachable!("the receiver opens it: {e:?}"));
    assert_eq!(header.kind, KIND_COMMAND_START);
    assert_eq!(header.layer, LayerId(LAYER));
    assert_eq!(header.dir, Direction::ToTerminal);
    assert_eq!(plain, 1u32.to_be_bytes());
    assert_eq!(layer.take_reply(), None, "an answer is taken once");
    assert_eq!(layer.counters().sealed, 1);
}

#[test]
fn a_prompt_and_a_command_end_are_sealed_with_their_payloads() {
    let mut layer = live();
    let mut rx = receiver();
    assert_eq!(ask(&mut layer, 1, KIND_PROMPT, &[]), Outcome::Sealed);
    let reply = layer.take_reply().unwrap_or_default();
    let (header, plain) = rx
        .open(0, &frame_of(&reply, 1))
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!((header.kind, plain.len()), (KIND_PROMPT, 0));

    let mut end = 1u32.to_be_bytes().to_vec();
    end.extend_from_slice(&(-1i32).to_be_bytes());
    assert_eq!(ask(&mut layer, 2, KIND_COMMAND_END, &end), Outcome::Sealed);
    let reply = layer.take_reply().unwrap_or_default();
    let (header, plain) = rx
        .open(0, &frame_of(&reply, 2))
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(header.kind, KIND_COMMAND_END);
    assert_eq!(
        plain, end,
        "the exit status is carried as the shell gave it"
    );
}

#[test]
fn sequence_numbers_only_go_up_and_a_replayed_frame_is_refused_by_the_receiver() {
    let mut layer = live();
    let mut rx = receiver();
    let mut frames = Vec::new();
    for n in 0..3 {
        assert_eq!(ask(&mut layer, n, KIND_PROMPT, &[]), Outcome::Sealed);
        let reply = layer.take_reply().unwrap_or_default();
        frames.push(frame_of(&reply, n));
    }
    let seqs: Vec<u64> = frames
        .iter()
        .map(|f| {
            let (h, _) = rx.open(0, f).unwrap_or_else(|e| unreachable!("{e:?}"));
            h.seq
        })
        .collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert_eq!(
        rx.open(0, &frames[0]).err(),
        Some(PayloadError::Header(HeaderError::Replay))
    );
}

#[test]
fn every_other_type_is_refused_and_uses_no_sequence_number() {
    let mut layer = live();
    let not_allowed = [
        (KIND_LAYOUT, vec![]),
        (KIND_PANE_CONTAINER, vec![]),
        (KIND_GRANT, vec![1, 8]),
        (KIND_CLIPBOARD_WRITE, b"text".to_vec()),
        (KIND_NOTIFICATION, vec![0, 2, b'h', b'i']),
        (KIND_MARKER, vec![0; 36]),
        (0x7f, vec![]),
        (0x80, vec![]),
        (0xff, vec![]),
    ];
    for (n, (kind, body)) in not_allowed.iter().enumerate() {
        let n = u32::try_from(n).unwrap_or(0);
        assert_eq!(
            ask(&mut layer, n, *kind, body),
            Outcome::SealRefused(SealRefusal::KindNotAllowed),
            "type {kind:#x}"
        );
        let reply = layer.take_reply().unwrap_or_default();
        assert_eq!(refusal_of(&reply, n), SealRefusal::KindNotAllowed as u8);
    }
    // The next sealed frame still carries the first sequence number.
    assert_eq!(ask(&mut layer, 99, KIND_PROMPT, &[]), Outcome::Sealed);
    let reply = layer.take_reply().unwrap_or_default();
    let (header, _) = receiver()
        .open(0, &frame_of(&reply, 99))
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(header.seq, 1);
    let c = layer.counters();
    assert_eq!((c.sealed, c.seal_refused), (1, 9));
}

#[test]
fn a_payload_that_is_not_the_layout_of_its_type_is_not_sealed() {
    let mut layer = live();
    for (kind, body) in [
        (KIND_COMMAND_START, vec![0, 0, 1]),
        (KIND_COMMAND_START, vec![0, 0, 0, 0, 1]),
        (KIND_PROMPT, vec![0]),
        (KIND_COMMAND_END, vec![0; 7]),
        (KIND_COMMAND_END, vec![0; 9]),
    ] {
        assert_eq!(
            ask(&mut layer, 1, kind, &body),
            Outcome::SealRefused(SealRefusal::BadBody),
            "type {kind:#x} with {} bytes",
            body.len()
        );
        let reply = layer.take_reply().unwrap_or_default();
        assert_eq!(refusal_of(&reply, 1), SealRefusal::BadBody as u8);
    }
    assert_eq!(layer.counters().sealed, 0);
}

#[test]
fn nothing_is_sealed_before_ready_or_before_the_key() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    assert_eq!(
        ask(&mut layer, 1, KIND_PROMPT, &[]),
        Outcome::SealRefused(SealRefusal::NotReady)
    );
    layer.handle(FROM_SHELL, &protocol::ready(2), false, &clock(99_000));
    assert_eq!(
        ask(&mut layer, 2, KIND_PROMPT, &[]),
        Outcome::SealRefused(SealRefusal::NoKey),
        "ready but no key"
    );
    let key = protocol::key_message(1, LAYER, &ROOT);
    layer.handle_spawn(FROM_TERMINAL, &key[..], false, &|_| true);
    assert_eq!(ask(&mut layer, 3, KIND_PROMPT, &[]), Outcome::Sealed);

    // A key with no ready is still not enough.
    let mut keyed = Layer::new(SHELL, TERMINAL);
    keyed.handle_spawn(FROM_TERMINAL, &key[..], false, &|_| true);
    assert_eq!(
        ask(&mut keyed, 1, KIND_PROMPT, &[]),
        Outcome::SealRefused(SealRefusal::NotReady)
    );
}

#[test]
fn a_request_from_anyone_but_the_shell_is_refused_unread_and_unanswered() {
    let mut layer = live();
    let m = protocol::seal_request(1, KIND_PROMPT, &[]).unwrap_or_else(|| unreachable!("fits"));
    for creds in [FROM_CHILD, None] {
        assert!(matches!(
            layer.handle(creds, &m, false, &clock(99_000)),
            Outcome::Refused(_)
        ));
        assert_eq!(layer.take_reply(), None, "no answer goes to a stranger");
    }
    assert_eq!(layer.counters().sealed, 0);
    assert_eq!(layer.counters().refused, 2);
}

#[test]
fn a_second_key_cannot_restart_the_sequence() {
    let mut layer = live();
    assert_eq!(ask(&mut layer, 1, KIND_PROMPT, &[]), Outcome::Sealed);
    let _ = layer.take_reply();
    let second = protocol::key_message(2, LAYER, &ROOT);
    layer.handle_spawn(FROM_TERMINAL, &second[..], false, &|_| true);
    assert_eq!(ask(&mut layer, 2, KIND_PROMPT, &[]), Outcome::Sealed);
    let reply = layer.take_reply().unwrap_or_default();
    let (header, _) = receiver()
        .open(0, &frame_of(&reply, 2))
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(header.seq, 2, "a nonce is never reused");
}

#[test]
fn every_answer_fits_one_message_and_never_holds_the_key() {
    let mut layer = live();
    let mut end = 1u32.to_be_bytes().to_vec();
    end.extend_from_slice(&i32::MAX.to_be_bytes());
    for (kind, body) in [
        (KIND_COMMAND_START, 1u32.to_be_bytes().to_vec()),
        (KIND_PROMPT, vec![]),
        (KIND_COMMAND_END, end),
    ] {
        assert_eq!(ask(&mut layer, 5, kind, &body), Outcome::Sealed);
        let reply = layer.take_reply().unwrap_or_default();
        assert!(reply.len() <= MAX_MESSAGE, "{} bytes", reply.len());
        assert!(
            !reply.windows(ROOT.len()).any(|w| w == ROOT),
            "the layer's root is never in an answer"
        );
    }
}

#[test]
fn a_request_has_a_header_a_frame_type_and_a_payload_up_to_the_message_limit() {
    let m =
        protocol::seal_request(0x0102_0304, 0x10, &[9, 9]).unwrap_or_else(|| unreachable!("fits"));
    assert_eq!(m, [VERSION, protocol::SEAL, 1, 2, 3, 4, 0x10, 9, 9]);
    assert_eq!(
        protocol::parse(&m, false),
        Ok(protocol::Message::Seal {
            number: 0x0102_0304,
            kind: 0x10,
            body: vec![9, 9]
        })
    );
    let full = protocol::seal_request(1, 0x10, &[0; MAX_SEAL_BODY]);
    assert_eq!(full.map(|m| m.len()), Some(MAX_MESSAGE));
    assert_eq!(
        protocol::seal_request(1, 0x10, &[0; MAX_SEAL_BODY + 1]),
        None
    );
    // Too short for the number and the type.
    assert_eq!(
        protocol::parse(&[VERSION, protocol::SEAL, 0, 0, 0, 1], false),
        Err(protocol::Malformed::Length)
    );
}
