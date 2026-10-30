//! The terminal's side of a sealed layer against a real holder: the layer's key
//! is made and delivered, the shell asks for a frame, and the terminal's layer
//! reads what the holder sealed. This process plays the shell.

#![cfg(all(target_os = "linux", feature = "seal-requests"))]

use lumen_arm_b::{Budget, Config, Context, Item, Layer};
use lumen_detector::TAG;
use lumen_holder::ffi_linux::{Received, pidfd_open, receive, wait_with_timeout};
use lumen_holder::protocol::{self, SEALED, SEALED_HEADER_LEN, seal_request};
use lumen_holder::terminal::{start_layer, start_layer_with};
use lumen_seal::{Decision, Reason};
use lumen_sealed::Receiver;
use lumen_sealed::types::{KIND_COMMAND_START, KIND_PROMPT, capability_of};
use std::io::Write as _;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

fn holder() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_lumen-holder"))
}

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

fn layer_with(receiver: Receiver) -> Layer {
    let mut l = Layer::new(Config {
        budget: Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    l.attach_sealed(receiver, cap_of);
    l.open_container();
    l
}

fn send(fd: &OwnedFd, bytes: &[u8]) {
    let mut file = std::fs::File::from(fd.try_clone().unwrap_or_else(|e| unreachable!("{e}")));
    assert!(file.write_all(bytes).is_ok());
}

fn answer(fd: &OwnedFd) -> Vec<u8> {
    let raw = fd.as_raw_fd();
    for _ in 0..100 {
        let events = wait_with_timeout(&[raw], 100).unwrap_or_default();
        if events.iter().any(|e| e.readable)
            && let Ok(Received::Message { bytes, .. }) = receive(raw, 256)
        {
            return bytes.to_vec();
        }
    }
    unreachable!("the holder did not answer");
}

/// The sealed frame as the shell puts it on the terminal's input.
fn on_the_wire(sealed: &[u8]) -> Vec<u8> {
    let kind = sealed[14];
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(sealed.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(sealed);
    v
}

/// A live layer for which this process is the shell.
fn live(layer: u32) -> (lumen_holder::spawn::Started, Receiver, OwnedFd) {
    let pidfd = pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("pidfd_open: {e}"));
    let (started, receiver) = start_layer(
        &Decision::Seal,
        holder(),
        &pidfd,
        std::process::id(),
        std::process::id(),
        layer,
    )
    .unwrap_or_else(|e| unreachable!("the layer starts: {e}"));
    (started, receiver, pidfd)
}

fn ask(started: &lumen_holder::spawn::Started, number: u32, kind: u8, body: &[u8]) -> Vec<u8> {
    send(
        &started.seal,
        &seal_request(number, kind, body).unwrap_or_else(|| unreachable!("fits")),
    );
    let reply = answer(&started.seal);
    assert_eq!(reply[1], SEALED, "{reply:?}");
    reply[SEALED_HEADER_LEN..].to_vec()
}

fn sealed_items(items: Vec<Item>) -> Vec<(u8, Vec<u8>)> {
    items
        .into_iter()
        .filter_map(|i| match i {
            Item::Sealed { header, body } => Some((header.kind, body)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_layer_that_failed_a_sealing_condition_starts_no_holder_and_makes_no_key() {
    use std::cell::Cell;
    let pidfd = pidfd_open(std::process::id()).unwrap_or_else(|e| unreachable!("{e}"));
    let asked_for_a_key = Cell::new(false);
    // A program that does not exist: if the layer tried to start a holder, the
    // error would be "not found", not the refusal.
    let nowhere = Path::new("/nonexistent/lumen-holder");
    for reason in [Reason::NotLinux, Reason::ShellGone] {
        let refused = start_layer_with(
            &Decision::GridOnly(reason),
            nowhere,
            &pidfd,
            std::process::id(),
            std::process::id(),
            1,
            &|| {
                asked_for_a_key.set(true);
                Ok(zeroize::Zeroizing::new([0u8; 32]))
            },
        );
        let Err(e) = refused else {
            unreachable!("a grid-only layer must not start a holder")
        };
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert!(e.to_string().contains("grid-only"), "{e}");
    }
    assert!(
        !asked_for_a_key.get(),
        "no key was made for a refused layer"
    );
    // The same call with `Seal` does reach for the program and the key.
    let Err(e) = start_layer_with(
        &Decision::Seal,
        nowhere,
        &pidfd,
        std::process::id(),
        std::process::id(),
        1,
        &|| {
            asked_for_a_key.set(true);
            Ok(zeroize::Zeroizing::new([0u8; 32]))
        },
    ) else {
        unreachable!("there is no such program")
    };
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound, "{e}");
    assert!(
        asked_for_a_key.get(),
        "a sealing layer asks for its key first"
    );
}

#[test]
fn the_terminals_layer_reads_the_frames_the_holder_seals_for_the_shell() {
    let (started, receiver, _pidfd) = live(3);
    let mut layer = layer_with(receiver);
    send(&started.seal, &protocol::ready(1));

    let start = ask(&started, 1, KIND_COMMAND_START, &1u32.to_be_bytes());
    let prompt = ask(&started, 2, KIND_PROMPT, &[]);
    let ctx = Context {
        now_ms: 10,
        ..Context::default()
    };
    let got = sealed_items(layer.feed(&on_the_wire(&start), ctx));
    assert_eq!(got, [(KIND_COMMAND_START, 1u32.to_be_bytes().to_vec())]);
    let got = sealed_items(layer.feed(&on_the_wire(&prompt), ctx));
    assert_eq!(got, [(KIND_PROMPT, vec![])]);

    // The same bytes again are a replay: the layer's receiver refuses them.
    assert_eq!(
        sealed_items(layer.feed(&on_the_wire(&start), ctx)),
        Vec::<(u8, Vec<u8>)>::new()
    );
}

#[test]
fn a_frame_changed_in_flight_is_not_a_sealed_item() {
    let (started, receiver, _pidfd) = live(4);
    let mut layer = layer_with(receiver);
    send(&started.seal, &protocol::ready(1));
    let mut frame = ask(&started, 1, KIND_PROMPT, &[]);
    let last = frame.len() - 1;
    frame[last] ^= 1;
    let ctx = Context::default();
    assert_eq!(
        sealed_items(layer.feed(&on_the_wire(&frame), ctx)),
        Vec::<(u8, Vec<u8>)>::new()
    );
}

#[test]
fn another_layers_receiver_does_not_read_this_holders_frames() {
    let (a, _receiver_a, _pa) = live(5);
    let (_b, receiver_b, _pb) = live(5);
    let mut layer_b = layer_with(receiver_b);
    send(&a.seal, &protocol::ready(1));
    let frame = ask(&a, 1, KIND_PROMPT, &[]);
    assert!(
        sealed_items(layer_b.feed(&on_the_wire(&frame), Context::default())).is_empty(),
        "each layer's key is its own, even with the same layer id"
    );
}
