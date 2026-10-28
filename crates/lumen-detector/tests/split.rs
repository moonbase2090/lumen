//! Detector behaviour across read boundaries.

use lumen_detector::{Detector, Event, TAG};

const INTRODUCERS: [&[u8]; 3] = [b"\x1b_", b"\xc2\x9f", b"\x9f"];

fn frame(intro: &[u8], kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = intro.to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(
        &u32::try_from(payload.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    v.extend_from_slice(payload);
    v
}

/// Run `stream` through a detector in the given chunks and merge adjacent
/// output events, since chunking may split ordinary output.
fn run(stream: &[u8], chunks: &[usize]) -> Vec<Event> {
    let mut d = Detector::default();
    let mut raw = Vec::new();
    let mut at = 0;
    for &n in chunks {
        d.feed(&stream[at..at + n], &mut raw);
        at += n;
    }
    d.feed(&stream[at..], &mut raw);
    d.finish(&mut raw);
    let mut merged: Vec<Event> = Vec::new();
    for e in raw {
        if let (Event::Output(new), Some(Event::Output(last)))
        | (Event::Escaped(new), Some(Event::Escaped(last))) = (&e, merged.last_mut())
        {
            last.extend_from_slice(new);
        } else {
            merged.push(e);
        }
    }
    merged
}

fn expect_whole(stream: &[u8]) -> Vec<Event> {
    run(stream, &[])
}

#[test]
fn every_introducer_yields_the_frame_and_passes_other_bytes() {
    for intro in INTRODUCERS {
        let mut s = b"before ".to_vec();
        s.extend(frame(intro, 7, b"hello"));
        s.extend_from_slice(b" after");
        assert_eq!(
            expect_whole(&s),
            vec![
                Event::Output(b"before ".to_vec()),
                Event::Frame {
                    kind: 7,
                    payload: b"hello".to_vec()
                },
                Event::Output(b" after".to_vec()),
            ],
            "introducer {intro:?}"
        );
    }
}

#[test]
fn every_single_split_position_matches_the_unsplit_result() {
    for intro in INTRODUCERS {
        let mut s = b"a\x1b[1mb ".to_vec();
        s.extend(frame(intro, 3, b"payload bytes"));
        s.extend_from_slice(b"\x1bc tail");
        let whole = expect_whole(&s);
        for cut in 1..s.len() {
            assert_eq!(run(&s, &[cut]), whole, "introducer {intro:?} cut {cut}");
        }
    }
}

#[test]
fn byte_at_a_time_matches_the_unsplit_result() {
    for intro in INTRODUCERS {
        let mut s = b"x".to_vec();
        s.extend(frame(intro, 1, b""));
        s.extend(frame(intro, 2, b"two"));
        s.push(b'y');
        let whole = expect_whole(&s);
        let ones = vec![1; s.len() - 1];
        assert_eq!(run(&s, &ones), whole, "introducer {intro:?}");
    }
}

#[test]
fn frame_bytes_never_appear_in_output() {
    for intro in INTRODUCERS {
        let s = frame(intro, 9, b"secret-payload");
        for cut in 1..s.len() {
            for e in run(&s, &[cut]) {
                if let Event::Output(bytes) = e {
                    assert!(bytes.is_empty(), "leaked {bytes:?} at cut {cut}");
                }
            }
        }
    }
}

#[test]
fn partial_introducer_or_header_at_end_of_stream_is_withheld_or_released() {
    let mut s = b"ab\x1b".to_vec();
    s.extend_from_slice(b"[0m");
    assert_eq!(expect_whole(&s), vec![Event::Output(s.clone())]);
    // A lone ESC or a lone 0xC2 is not an introducer, so it is ordinary output.
    for tail in [&b"\x1b"[..], b"\xc2"] {
        let mut s = b"ab".to_vec();
        s.extend_from_slice(tail);
        assert_eq!(
            expect_whole(&s),
            vec![Event::Output(s.clone())],
            "tail {tail:?}"
        );
    }
    // A complete introducer is withheld; the partial header after it is shown
    // as escaped text.
    let header_short = frame(b"\x1b_", 1, b"xyz");
    for (tail, rest) in [
        (&b"\x1b_"[..], &b""[..]),
        (b"\xc2\x9f", b""),
        (b"\x9f", b""),
        (&header_short[..6], &header_short[2..6]),
    ] {
        let mut s = b"ab".to_vec();
        s.extend_from_slice(tail);
        let mut want = vec![Event::Output(b"ab".to_vec())];
        if !rest.is_empty() {
            want.push(Event::Escaped(rest.to_vec()));
        }
        assert_eq!(expect_whole(&s), want, "tail {tail:?}");
    }
}

#[test]
fn wrong_tag_is_shown_as_escaped_text_and_scanning_resumes_inside_it() {
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(b"NOTATAG!");
    s.extend(frame(b"\x1b_", 5, b"ok"));
    let got = expect_whole(&s);
    assert_eq!(
        got,
        vec![
            Event::Escaped(b"NOTATAG!".to_vec()),
            Event::Frame {
                kind: 5,
                payload: b"ok".to_vec()
            },
        ]
    );
    // An introducer that starts inside a failed candidate is still found.
    let mut s = b"\x1b".to_vec();
    s.extend(frame(b"\x1b_", 6, b"in"));
    assert_eq!(
        expect_whole(&s),
        vec![
            Event::Output(b"\x1b".to_vec()),
            Event::Frame {
                kind: 6,
                payload: b"in".to_vec()
            },
        ]
    );
}

#[test]
fn oversize_header_is_rejected_and_the_body_is_scanned_not_skipped() {
    let mut d = Detector::new(8);
    let mut s = frame(b"\x1b_", 4, &[0u8; 9]);
    let header_len = 2 + TAG.len() + 1 + 4;
    // The declared length is 9, which is over the limit of 8.
    s.extend(frame(b"\x1b_", 5, b"fine"));
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    d.finish(&mut out);
    assert_eq!(out[0], Event::Rejected(s[..header_len].to_vec()));
    // The failed header is not a frame, so what follows it is scanned from the
    // byte after the introducer: the rest of the header and the body are
    // ordinary output, and the next real frame is still found.
    assert_eq!(
        out.last(),
        Some(&Event::Frame {
            kind: 5,
            payload: b"fine".to_vec()
        })
    );
    assert!(out.iter().all(|e| !matches!(e, Event::Truncated(_))));
}

#[test]
fn frame_hidden_in_a_rejected_header_is_found() {
    // Outer header: introducer, tag, kind, then a length field that starts with
    // the introducer of an inner frame whose tag runs on past the field.
    let inner = frame(b"\x1b_", 8, b"hid");
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(&TAG);
    s.push(1);
    s.extend_from_slice(&inner[..4]);
    s.extend_from_slice(&inner[4..]);
    let mut d = Detector::new(8);
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    d.finish(&mut out);
    assert!(
        out.contains(&Event::Frame {
            kind: 8,
            payload: b"hid".to_vec()
        }),
        "{out:?}"
    );
}

#[test]
fn rejected_body_bytes_do_not_skip_following_input() {
    // A huge declared length must not swallow the rest of the stream.
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(&TAG);
    s.push(1);
    s.extend_from_slice(&u32::MAX.to_be_bytes());
    s.extend_from_slice(b"visible");
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    assert!(matches!(out[0], Event::Rejected(_)));
    let tail: Vec<u8> = out[1..]
        .iter()
        .flat_map(|e| match e {
            Event::Output(b) => b.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert!(tail.ends_with(b"visible"));
}

#[test]
fn stream_ending_inside_a_frame_is_reported_truncated() {
    let s = frame(b"\x1b_", 2, b"abcdef");
    let cut = &s[..s.len() - 3];
    assert_eq!(
        expect_whole(cut),
        vec![Event::Truncated(cut[2 + TAG.len() + 5..].to_vec())]
    );
}

/// A small xorshift generator so the test needs no dependency and is repeatable.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
    }
}

#[test]
fn random_streams_in_random_chunks_match_the_unsplit_result() {
    let pieces: [&[u8]; 8] = [
        b"plain text ",
        b"\x1b[31m",
        b"\x1b",
        b"\x1b_",
        b"\xc2",
        b"\x9f",
        b"\xc2\x9f",
        &TAG,
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..3000 {
        let mut s = Vec::new();
        for _ in 0..rng.below(12) {
            if rng.below(3) == 0 {
                let intro = INTRODUCERS[rng.below(3)];
                let len = rng.below(20);
                let payload: Vec<u8> = (0..len)
                    .map(|_| u8::try_from(rng.below(256)).unwrap_or(0))
                    .collect();
                s.extend(frame(
                    intro,
                    u8::try_from(rng.below(256)).unwrap_or(0),
                    &payload,
                ));
            } else {
                s.extend_from_slice(pieces[rng.below(pieces.len())]);
            }
        }
        let whole = expect_whole(&s);
        let mut cuts = Vec::new();
        let mut left = s.len();
        while left > 0 && rng.below(3) != 0 {
            let n = 1 + rng.below(left);
            cuts.push(n);
            left -= n;
        }
        assert_eq!(run(&s, &cuts), whole, "case {case} cuts {cuts:?}");
    }
}

/// A header for an unsealed frame that declares `declared` bytes.
fn header(kind: u8, declared: u32) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&declared.to_be_bytes());
    v
}

#[test]
fn an_unsealed_header_cannot_hide_a_later_frame() {
    for inner_intro in INTRODUCERS {
        let inner = frame(inner_intro, 5, b"genuine");
        let mut s = header(0x80, 500);
        s.extend_from_slice(b"abc");
        s.extend_from_slice(&inner);
        s.extend_from_slice(b"after");
        let want = vec![
            Event::Truncated(b"abc".to_vec()),
            Event::Frame {
                kind: 5,
                payload: b"genuine".to_vec(),
            },
            Event::Output(b"after".to_vec()),
        ];
        // Whole, byte by byte, and every single split position.
        assert_eq!(run(&s, &[]), want, "whole, introducer {inner_intro:?}");
        assert_eq!(
            run(&s, &vec![1; s.len() - 1]),
            want,
            "one byte at a time, introducer {inner_intro:?}"
        );
        for at in 1..s.len() {
            assert_eq!(
                run(&s, &[at]),
                want,
                "split {at}, introducer {inner_intro:?}"
            );
        }
    }
}

#[test]
fn a_trigger_at_the_start_of_the_payload_leaves_an_empty_truncated_frame() {
    let mut s = header(0x80, 100);
    s.extend(frame(b"\x1b_", 5, b"x"));
    assert_eq!(
        run(&s, &[]),
        vec![
            Event::Truncated(Vec::new()),
            Event::Frame {
                kind: 5,
                payload: b"x".to_vec()
            }
        ]
    );
}

#[test]
fn bare_introducer_bytes_inside_an_unsealed_payload_are_payload() {
    // A 159-byte string has the length bytes 00 9F; a 27-byte string that starts
    // with `_` has 00 1B 5F; U+27DF is E2 9F 9F. None may end the frame.
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    let mut p = vec![0, 0x9f];
    p.extend(vec![b'a'; 159]);
    payloads.push(p);
    let mut p = vec![0, 0x1b, b'_'];
    p.extend(vec![b'b'; 26]);
    payloads.push(p);
    let mut p = vec![0, 3, 0xe2, 0x9f, 0x9f];
    p.extend_from_slice(b"\xc2\x9f");
    payloads.push(p);
    for payload in payloads {
        let s = frame(b"\x1b_", 0x80, &payload);
        let want = vec![Event::Frame {
            kind: 0x80,
            payload: payload.clone(),
        }];
        assert_eq!(run(&s, &[]), want);
        assert_eq!(run(&s, &vec![1; s.len() - 1]), want);
        for at in 1..s.len() {
            assert_eq!(run(&s, &[at]), want, "split {at}");
        }
    }
}

#[test]
fn a_sealed_range_region_is_not_scanned_for_triggers() {
    // The length of a sealed-range frame is only used once its header has been
    // accepted, so its region is not rescanned here.
    let inner = frame(b"\x1b_", 5, b"in");
    let mut payload = b"ab".to_vec();
    payload.extend_from_slice(&inner);
    let s = frame(b"\x1b_", 3, &payload);
    assert_eq!(run(&s, &[]), vec![Event::Frame { kind: 3, payload }]);
}
