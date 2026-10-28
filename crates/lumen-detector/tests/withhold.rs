//! A non-frame candidate's introducer never reaches the VT parser, and the
//! genuine output that follows still does. The VT parser here is the real one:
//! it treats `ESC _` as the start of a string and discards output until the
//! string ends, so an introducer that leaked through would hide what follows.

use lumen_detector::{Detector, Event, HeaderCheck, TAG};
use vte::{Params, Parser, Perform};

const INTRODUCERS: [&[u8]; 3] = [b"\x1b_", b"\xc2\x9f", b"\x9f"];

#[derive(Default)]
struct Screen(String);

impl Perform for Screen {
    fn print(&mut self, c: char) {
        self.0.push(c);
    }
    fn execute(&mut self, _byte: u8) {}
    fn hook(&mut self, _: &Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {}
    fn csi_dispatch(&mut self, _: &Params, _: &[u8], _: bool, _: char) {}
    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {}
}

/// What the VT parser prints for the bytes the detector passed on.
fn printed(events: &[Event], follow: &[u8]) -> String {
    let mut bytes: Vec<u8> = events
        .iter()
        .flat_map(|e| match e {
            Event::Output(b) => b.clone(),
            _ => Vec::new(),
        })
        .collect();
    bytes.extend_from_slice(follow);
    let mut parser = Parser::new();
    let mut screen = Screen::default();
    parser.advance(&mut screen, &bytes);
    screen.0
}

struct RefuseAll;

impl HeaderCheck for RefuseAll {
    fn prefix_len(&self, kind: u8) -> usize {
        if kind < 0x80 { 4 } else { 0 }
    }
    fn accept(&mut self, _: u8, _: usize, _: &[u8]) -> bool {
        false
    }
}

fn frame(intro: &[u8], kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = intro.to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

#[test]
fn wrong_tag_introducer_is_withheld() {
    for intro in INTRODUCERS {
        let mut s = intro.to_vec();
        s.extend_from_slice(b"NOTATAG!visible");
        let mut d = Detector::default();
        let mut out = Vec::new();
        d.feed(&s, &mut out);
        d.finish(&mut out);
        assert!(printed(&out, b"").contains("visible"), "{intro:?}");
    }
}

#[test]
fn refused_header_introducer_is_withheld() {
    for intro in INTRODUCERS {
        let mut s = frame(intro, 3, b"BAD!more bytes");
        s.extend_from_slice(b"visible");
        let mut d = Detector::default();
        let mut out = Vec::new();
        d.feed_checked(&s, &mut out, &mut RefuseAll);
        d.finish(&mut out);
        assert!(printed(&out, b"").contains("visible"), "{intro:?}");
    }
}

#[test]
fn over_limit_introducer_is_withheld() {
    for intro in INTRODUCERS {
        let mut s = frame(intro, 4, &[7u8; 9]);
        s.extend_from_slice(b"visible");
        let mut d = Detector::new(8);
        let mut out = Vec::new();
        d.feed(&s, &mut out);
        d.finish(&mut out);
        assert!(printed(&out, b"").contains("visible"), "{intro:?}");
    }
}

#[test]
fn partial_introducer_at_end_of_stream_is_withheld() {
    for intro in INTRODUCERS {
        for cut in 0..TAG.len() {
            let mut s = intro.to_vec();
            s.extend_from_slice(&TAG[..cut]);
            let mut d = Detector::default();
            let mut out = Vec::new();
            d.feed(&s, &mut out);
            d.finish(&mut out);
            // The output that comes after the stream resumes must still print.
            assert!(
                printed(&out, b"visible").contains("visible"),
                "{intro:?} cut {cut}"
            );
        }
    }
}

#[test]
fn a_prefix_that_is_not_an_introducer_is_still_output() {
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.feed(b"a\x1b[31mred\x1b[0m b", &mut out);
    d.finish(&mut out);
    let all: Vec<u8> = out
        .iter()
        .flat_map(|e| match e {
            Event::Output(b) => b.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(all, b"a\x1b[31mred\x1b[0m b");
}

#[test]
fn utf8_text_with_a_0x9f_continuation_byte_is_not_damaged() {
    // U+00DF has 0x9F as its second byte; so do U+0178 and U+271F.
    let text = "ß Ÿ ✟ 日本語 𝟘 ünï";
    let bytes = text.as_bytes();
    assert!(bytes.contains(&0x9f));
    for cut in 0..=bytes.len() {
        let mut d = Detector::default();
        let mut out = Vec::new();
        d.feed(&bytes[..cut], &mut out);
        d.feed(&bytes[cut..], &mut out);
        d.finish(&mut out);
        let got: Vec<u8> = out
            .iter()
            .flat_map(|e| match e {
                Event::Output(b) => b.clone(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(got, bytes, "cut {cut}");
    }
}

#[test]
fn raw_c1_introducer_after_an_open_utf8_lead_byte_is_text_not_a_frame() {
    // Documented exception: 0x9F continues the sequence started by 0xC3.
    let mut s = vec![0xc3];
    s.extend_from_slice(&[0x9f]);
    s.extend_from_slice(&TAG);
    s.push(1);
    s.extend_from_slice(&0u32.to_be_bytes());
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.feed(&s, &mut out);
    d.finish(&mut out);
    assert!(
        out.iter().all(|e| !matches!(e, Event::Frame { .. })),
        "{out:?}"
    );
    // With no open sequence the same bytes are a frame.
    let mut s = vec![b'a', 0x9f];
    s.extend_from_slice(&TAG);
    s.push(1);
    s.extend_from_slice(&0u32.to_be_bytes());
    let mut out = Vec::new();
    let mut d = Detector::default();
    d.feed(&s, &mut out);
    assert!(
        out.iter()
            .any(|e| matches!(e, Event::Frame { kind: 1, .. })),
        "{out:?}"
    );
}
