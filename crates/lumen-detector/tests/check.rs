//! A header check decides acceptance before the declared length is trusted.

use lumen_detector::{Detector, Event, HeaderCheck, TAG};

/// Accepts a sealed-range frame (type below 0x80) only if it starts with `GOOD`.
struct Good {
    calls: usize,
}

impl HeaderCheck for Good {
    fn prefix_len(&self, kind: u8) -> usize {
        if kind < 0x80 { 4 } else { 0 }
    }

    fn accept(&mut self, _kind: u8, _declared: usize, prefix: &[u8]) -> bool {
        self.calls += 1;
        prefix == b"GOOD"
    }
}

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

fn header_only(kind: u8, declared: u32) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&declared.to_be_bytes());
    v
}

fn run(stream: &[u8], chunks: &[usize]) -> Vec<Event> {
    let mut d = Detector::default();
    let mut check = Good { calls: 0 };
    let mut raw = Vec::new();
    let mut at = 0;
    for &n in chunks {
        d.feed_checked(&stream[at..at + n], &mut raw, &mut check);
        at += n;
    }
    d.feed_checked(&stream[at..], &mut raw, &mut check);
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

#[test]
fn accepted_frame_comes_out_whole() {
    let s = frame(b"\x1b_", 3, b"GOOD body");
    assert_eq!(
        run(&s, &[]),
        vec![Event::Frame {
            kind: 3,
            payload: b"GOOD body".to_vec()
        }]
    );
}

#[test]
fn unsealed_range_frames_skip_the_check() {
    let s = frame(b"\x1b_", 0x80, b"anything");
    assert_eq!(
        run(&s, &[]),
        vec![Event::Frame {
            kind: 0x80,
            payload: b"anything".to_vec()
        }]
    );
}

#[test]
fn refused_header_does_not_skip_input_by_its_declared_length() {
    // A refused candidate declares far more bytes than follow it. A genuine
    // frame right after it must still be found, and the bytes in between come
    // out as ordinary output.
    let mut s = frame(b"\x1b_", 3, b"BAD!");
    let declared_at = 2 + TAG.len() + 1;
    s[declared_at..declared_at + 4].copy_from_slice(&1_000_000u32.to_be_bytes());
    s.extend(frame(b"\x1b_", 3, b"GOOD real"));
    let got = run(&s, &[]);
    assert!(matches!(got[0], Event::Rejected(_)), "{got:?}");
    assert_eq!(
        got.last(),
        Some(&Event::Frame {
            kind: 3,
            payload: b"GOOD real".to_vec()
        })
    );
}

#[test]
fn genuine_frame_hidden_inside_a_refused_one_is_found() {
    let inner = frame(b"\x1b_", 3, b"GOOD inner");
    let mut s = header_only(3, 500);
    s.extend_from_slice(b"BAD!");
    s.extend_from_slice(&inner);
    let got = run(&s, &[]);
    assert!(
        got.contains(&Event::Frame {
            kind: 3,
            payload: b"GOOD inner".to_vec()
        }),
        "{got:?}"
    );
}

#[test]
fn declared_length_shorter_than_the_checked_prefix_is_refused_at_once() {
    let mut s = frame(b"\x1b_", 3, b"GO");
    s.extend(frame(b"\x1b_", 3, b"GOOD next"));
    let got = run(&s, &[]);
    assert!(matches!(got[0], Event::Rejected(_)), "{got:?}");
    assert_eq!(
        got.last(),
        Some(&Event::Frame {
            kind: 3,
            payload: b"GOOD next".to_vec()
        })
    );
}

#[test]
fn nested_refused_candidates_do_not_overflow_the_stack() {
    // Many refused headers back to back, each hiding the next.
    let mut s = Vec::new();
    for _ in 0..20_000 {
        s.extend(header_only(3, 900));
    }
    s.extend(frame(b"\x1b_", 3, b"GOOD end"));
    let got = run(&s, &[]);
    assert_eq!(
        got.last(),
        Some(&Event::Frame {
            kind: 3,
            payload: b"GOOD end".to_vec()
        })
    );
}

#[test]
fn every_split_position_matches_the_unsplit_result_with_a_check() {
    let mut s = b"x".to_vec();
    s.extend(header_only(3, 700));
    s.extend_from_slice(b"BAD!more");
    s.extend(frame(b"\x1b_", 3, b"GOOD one"));
    s.extend(frame(b"\xc2\x9f", 0x81, b"two"));
    s.push(b'y');
    let whole = run(&s, &[]);
    for cut in 1..s.len() {
        assert_eq!(run(&s, &[cut]), whole, "cut {cut}");
    }
    let ones = vec![1; s.len() - 1];
    assert_eq!(run(&s, &ones), whole);
}
