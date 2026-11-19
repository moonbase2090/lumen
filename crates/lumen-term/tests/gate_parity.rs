//! Gate G14, side-channel parity: no Lumen channel accepts anything that PTY
//! writes could not already do ([`gate.parity`]).
//!
//! A program that writes to its PTY can already put text on the screen. So the
//! unsealed channel (Track 0 frames) may do no more than that: its bytes must
//! end as inert blocks or visible text, and never reach the VT parser. The
//! sealed channel may do more only for a writer that holds the key, so a program
//! that does not hold it must get nothing from it, however it forges, replays,
//! redirects or damages a real frame. Finally the terminal must not answer a
//! Lumen sequence by writing to the PTY, because that would be a channel in the
//! other direction.

#![cfg(unix)]

use lumen_detector::TAG;
use lumen_sealed::types::{KIND_GRANT, KIND_NOTIFICATION, capability_of};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Receiver, Sender};
use lumen_term::{Context, Document, Event, Layer, Piece, Session};
use std::time::{Duration, Instant};

const LAYER: LayerId = LayerId(3);
const ROOT: [u8; 32] = [4; 32];

fn keys_for(root: [u8; 32], layer: LayerId, dir: Direction) -> DirectionKeys {
    DirectionKeys::derive(&LayerRoot::new(root), layer, dir)
}

fn cap_of(kind: u8) -> u8 {
    capability_of(kind).unwrap_or(0)
}

fn layer() -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    l.attach_sealed(
        Receiver::new(
            keys_for(ROOT, LAYER, Direction::ToTerminal),
            LAYER,
            Direction::ToTerminal,
        ),
        cap_of,
    );
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

fn seal(sender: &mut Sender, kind: u8, payload: &[u8]) -> Vec<u8> {
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

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Bytes that cannot start a frame: no `ESC _`, and no 0x9f, the 8-bit APC
/// introducer, which the detector reads as a frame start by design.
fn plain_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut out: Vec<u8> = Vec::with_capacity(len);
    while out.len() < len {
        let b = rng.next().to_be_bytes()[3];
        if b == 0x9f || (b == b'_' && out.last() == Some(&0x1b)) {
            continue;
        }
        out.push(b);
    }
    out
}

/// Run `bytes` through the layer in reads of the given sizes, then finish.
fn run(bytes: &[u8], sizes: &[usize]) -> Vec<Piece> {
    let mut l = layer();
    let mut pieces = Vec::new();
    let mut at_byte = 0;
    let mut i = 0;
    while at_byte < bytes.len() {
        let end = (at_byte + sizes[i % sizes.len()]).min(bytes.len());
        pieces.extend(Layer::feed(&mut l, &bytes[at_byte..end], at(0)));
        at_byte = end;
        i += 1;
    }
    pieces.extend(Layer::finish(&mut l));
    pieces
}

fn output_of(pieces: &[Piece]) -> Vec<u8> {
    pieces
        .iter()
        .filter_map(|p| match p {
            Piece::Bytes(b) => Some(b.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

#[test]
fn bytes_that_cannot_start_a_frame_reach_the_parser_exactly_as_written() {
    for seed in 1..=40 {
        let bytes = plain_bytes(seed, 200 + usize::try_from(seed).unwrap_or(0) * 37);
        for sizes in [&[bytes.len()][..], &[1], &[3, 1, 7], &[64]] {
            let pieces = run(&bytes, sizes);
            assert_eq!(
                output_of(&pieces),
                bytes,
                "seed {seed}, reads {sizes:?}: the layer changed plain output"
            );
            assert!(
                pieces.iter().all(|p| matches!(p, Piece::Bytes(_))),
                "seed {seed}: plain bytes produced something other than bytes"
            );
        }
    }
}

#[test]
fn an_unsealed_frame_ends_as_a_block_or_visible_text_and_never_reaches_the_parser() {
    let payloads: Vec<Vec<u8>> = vec![
        Vec::new(),
        string("plain"),
        string("\x1b]52;c;c2VjcmV0\x07 \x1b[2J \u{9d}"),
        {
            let mut v = vec![1];
            v.extend(string("heading"));
            v
        },
        {
            let mut v = string("rust");
            v.extend(string("fn main() {}"));
            v
        },
        vec![0xff; 40],
    ];
    let mut frames = 0;
    for kind in 0..=255u8 {
        for payload in &payloads {
            let bytes = frame(kind, payload);
            for sizes in [&[bytes.len()][..], &[1]] {
                let pieces = run(&bytes, sizes);
                for p in &pieces {
                    match p {
                        Piece::Bytes(b) => assert!(
                            !b.windows(2).any(|w| w == b"\x1b_") && !b.contains(&0x9f),
                            "kind {kind:#04x}: frame bytes reached the parser: {b:?}"
                        ),
                        Piece::Text(t) => assert!(
                            !t.chars().any(char::is_control),
                            "kind {kind:#04x}: control character in visible text"
                        ),
                        Piece::Sealed { .. } => {
                            unreachable!("kind {kind:#04x}: an unsealed frame opened as sealed")
                        }
                        Piece::Rich(_) | Piece::Background(_) | Piece::Notice(_) => {}
                    }
                }
                // Whatever is not shown as a block is shown as text, so the
                // terminal does not silently eat the writer's bytes either.
                assert!(
                    !pieces.is_empty(),
                    "kind {kind:#04x}: a frame produced nothing at all"
                );
                frames += 1;
            }
        }
    }
    assert_eq!(frames, 256 * 6 * 2);
}

/// What a document made of a stream shows as sealed authority.
fn opened(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut d = Document::new(60, 8);
    let mut l = layer();
    d.receive(&mut l, bytes, at(0));
    d.finish(&mut l);
    d.take_events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Sealed { kind, body } => Some((kind, body)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_real_sealed_frame_opens_once_and_the_control_shows_the_oracle_works() {
    let mut sender = Sender::new(
        keys_for(ROOT, LAYER, Direction::ToTerminal),
        LAYER,
        Direction::ToTerminal,
        1,
    );
    let f = seal(&mut sender, KIND_NOTIFICATION, &string("hi"));
    let got = opened(&f);
    assert_eq!(got.len(), 1, "the genuine frame opens");
    assert_eq!(got[0].0, KIND_NOTIFICATION);
    let twice: Vec<u8> = [f.clone(), f.clone()].concat();
    assert_eq!(
        opened(&twice).len(),
        1,
        "a replay of the same frame is refused"
    );
}

#[test]
fn a_program_without_the_key_gets_nothing_from_the_sealed_channel() {
    let sender_with = |root: [u8; 32], layer: LayerId, dir: Direction, seq: u64| {
        Sender::new(keys_for(root, layer, dir), layer, dir, seq)
    };
    let note = string("you won");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        // Sealed under a key the layer does not hold.
        (
            "wrong-root",
            seal(
                &mut sender_with([9; 32], LAYER, Direction::ToTerminal, 1),
                KIND_NOTIFICATION,
                &note,
            ),
        ),
        // Right root, but for another layer: a frame moved between layers.
        (
            "other-layer",
            seal(
                &mut sender_with(ROOT, LayerId(4), Direction::ToTerminal, 1),
                KIND_NOTIFICATION,
                &note,
            ),
        ),
        // Right root and layer, wrong direction: a frame meant for the shell.
        (
            "wrong-direction",
            seal(
                &mut sender_with(ROOT, LAYER, Direction::ToShell, 1),
                KIND_NOTIFICATION,
                &note,
            ),
        ),
        // A grant: the frame a program would most want to forge.
        (
            "wrong-root-grant",
            seal(
                &mut sender_with([9; 32], LAYER, Direction::ToTerminal, 1),
                KIND_GRANT,
                &[1, 8],
            ),
        ),
    ];

    // Damage to a genuine frame at every byte position.
    let mut sender = sender_with(ROOT, LAYER, Direction::ToTerminal, 1);
    let genuine = seal(&mut sender, KIND_NOTIFICATION, &string("genuine"));
    // The sealed bytes start after ESC _ + TAG + kind + length.
    let start = 2 + TAG.len() + 1 + 4;
    assert!(genuine.len() > start);
    let mut flipped_cases = Vec::new();
    for i in start..genuine.len() {
        let mut f = genuine.clone();
        f[i] ^= 0x01;
        flipped_cases.push((format!("flip-byte-{}", i - start), f));
    }
    for cut in start..genuine.len() {
        // The length field still says the full length, so the frame is short.
        flipped_cases.push((format!("truncate-{}", cut - start), genuine[..cut].to_vec()));
    }
    let mut retyped = genuine.clone();
    retyped[2 + TAG.len()] = KIND_GRANT;
    flipped_cases.push(("kind-changed-outside-the-seal".into(), retyped));

    for (name, bytes) in &cases {
        assert_eq!(opened(bytes), [], "{name}: a frame opened");
    }
    for (name, bytes) in &flipped_cases {
        assert_eq!(opened(bytes), [], "{name}: a damaged frame opened");
    }

    // A replayed frame after the genuine one is refused at every position.
    let mut sender = sender_with(ROOT, LAYER, Direction::ToTerminal, 1);
    let first = seal(&mut sender, KIND_NOTIFICATION, &string("one"));
    let second = seal(&mut sender, KIND_NOTIFICATION, &string("two"));
    let stream: Vec<u8> = [second.clone(), first.clone(), second.clone()].concat();
    let got = opened(&stream);
    assert_eq!(got.len(), 1, "only the first delivery of `second` opens");
    assert_eq!(got[0].0, KIND_NOTIFICATION);
}

fn start_script(script: &str) -> Session {
    let l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Session::start(Box::new(l), 60, 6, "/bin/sh", &["-c", script])
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

fn grid_text(s: &Session) -> String {
    s.document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
            lumen_term::Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pump_until(s: &mut Session, done: impl Fn(&Session) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        s.pump_wait(Duration::from_millis(100));
        if done(s) {
            return true;
        }
    }
    false
}

/// A program that prints `queries` on its PTY, then reads for a second with the
/// line discipline in raw mode and writes whatever it received to `out`.
fn probe(name: &str, queries: &[u8], send_after_ready: Option<&[u8]>) -> usize {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let q = dir.join(format!("{name}.queries"));
    let out = dir.join(format!("{name}.received"));
    assert!(std::fs::write(&q, queries).is_ok());
    let _ = std::fs::remove_file(&out);
    let script = format!(
        "stty raw -echo; cat '{q}'; echo READY; stty min 0 time 15; \
         dd bs=1 count=256 of='{out}' 2>/dev/null; echo DONE",
        q = q.display(),
        out = out.display()
    );
    let mut s = start_script(&script);
    assert!(
        pump_until(&mut s, |s| grid_text(s).contains("READY")),
        "{name}: READY never appeared; the grid shows {:?}",
        grid_text(&s)
    );
    if let Some(bytes) = send_after_ready {
        assert!(s.send(bytes).is_ok());
    }
    assert!(
        pump_until(&mut s, |s| grid_text(s).contains("DONE")),
        "{name}: DONE never appeared; the grid shows {:?}",
        grid_text(&s)
    );
    std::fs::metadata(&out).map_or(0, |m| usize::try_from(m.len()).unwrap_or(usize::MAX))
}

#[test]
fn the_terminal_writes_nothing_back_for_any_lumen_sequence() {
    // The control: bytes the terminal is asked to send do reach the program, so
    // a reply, had there been one, would have been counted.
    assert_eq!(
        probe("parity-control", b"nothing to see\n", Some(b"xyz")),
        3
    );

    let mut queries = Vec::new();
    for s in [
        &b"\x1b]9;hello\x07"[..],
        b"\x1b]777;notify;title;body\x07",
        b"\x1b]99;i=1:p=?;\x1b\\",
        b"\x1b]99;i=2:a=report:p=close;\x07",
        b"\x1b]99;i=3;hello\x1b\\",
        b"\x1b]52;c;?\x07",
        b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\",
    ] {
        queries.extend_from_slice(s);
    }
    // Frames in the unsealed range and a forged sealed one.
    queries.extend(frame(0x80, &string("hello")));
    queries.extend(frame(KIND_NOTIFICATION, &string("forged")));
    assert_eq!(
        probe("parity-queries", &queries, None),
        0,
        "the terminal wrote bytes to the PTY in answer to a Lumen sequence"
    );
}
