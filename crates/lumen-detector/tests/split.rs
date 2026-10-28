//! Detector behaviour across read boundaries.

use lumen_detector::{DEFAULT_MAX_PAYLOAD, Detector, Event, TAG};

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
        if let (Event::Output(new), Some(Event::Output(last))) = (&e, merged.last_mut()) {
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
fn partial_introducer_that_never_completes_is_ordinary_output() {
    let mut s = b"ab\x1b".to_vec();
    s.extend_from_slice(b"[0m");
    assert_eq!(expect_whole(&s), vec![Event::Output(s.clone())]);
    for tail in [&b"\x1b_"[..], b"\xc2", b"\x9f"] {
        let mut s = b"ab".to_vec();
        s.extend_from_slice(tail);
        // Ends in the middle of an introducer: released as output at the end.
        let got = expect_whole(&s);
        if tail == b"\x1b_" || tail == b"\x9f" {
            assert_eq!(got.last(), Some(&Event::Truncated(tail.to_vec())));
        } else {
            assert_eq!(got, vec![Event::Output(s.clone())]);
        }
    }
}

#[test]
fn wrong_tag_is_ordinary_output_and_scanning_resumes_inside_it() {
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(b"NOTATAG!");
    s.extend(frame(b"\x1b_", 5, b"ok"));
    let got = expect_whole(&s);
    assert_eq!(
        got,
        vec![
            Event::Output(b"\x1b_NOTATAG!".to_vec()),
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
fn oversize_declared_length_is_rejected_and_detector_resumes() {
    let mut d = Detector::new(8);
    let mut s = frame(b"\x1b_", 4, &[0u8; 9]);
    let header_len = 2 + TAG.len() + 1 + 4;
    s.extend(frame(b"\x1b_", 5, b"fine"));
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    d.finish(&mut out);
    assert_eq!(out[0], Event::Rejected(s[..header_len].to_vec()));
    // The aborted frame's body arrives as ordinary output, then the next frame.
    assert_eq!(out[1], Event::Output(vec![0u8; 9]));
    assert_eq!(
        out[2],
        Event::Frame {
            kind: 5,
            payload: b"fine".to_vec()
        }
    );
    assert_eq!(out.len(), 3);
}

#[test]
fn declared_length_past_u32_range_on_this_platform_is_rejected() {
    let mut s = b"\x1b_".to_vec();
    s.extend_from_slice(&TAG);
    s.push(1);
    s.extend_from_slice(&u32::MAX.to_be_bytes());
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    assert_eq!(out, vec![Event::Rejected(s)]);
    assert!(usize::try_from(u32::MAX).map_or(true, |n| n > DEFAULT_MAX_PAYLOAD));
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
