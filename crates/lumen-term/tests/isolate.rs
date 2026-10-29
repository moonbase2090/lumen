//! Grid parser isolation, rule by rule. The fuzzer over every introducer at
//! every split position is in `isolation_fuzz.rs`.

use lumen_term::isolate::{Isolator, Note, STRING_CAP, Stats};

fn run(chunks: &[&[u8]]) -> (Vec<u8>, Vec<Note>, Stats) {
    let mut f = Isolator::new();
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for c in chunks {
        f.filter(c, &mut out, &mut notes);
    }
    f.finish();
    (out, notes, f.stats())
}

fn once(bytes: &[u8]) -> Vec<u8> {
    run(&[bytes]).0
}

#[test]
fn plain_text_and_ordinary_escapes_pass_unchanged() {
    let s =
        b"ls -la\r\n\x1b[31mred\x1b[0m \x1b[2J\x1b[1;1H caf\xc3\xa9 \xe2\x82\xac \xf0\x9f\x98\x80";
    assert_eq!(once(s), s);
}

#[test]
fn string_escapes_are_dropped_in_every_7_bit_form() {
    for intro in [&b"\x1bP"[..], b"\x1b_", b"\x1b^", b"\x1bX"] {
        let mut s = b"a".to_vec();
        s.extend_from_slice(intro);
        s.extend_from_slice(b"payload\x1b\\b");
        let (out, _, stats) = run(&[&s]);
        assert_eq!(out, b"ab", "{intro:?}");
        assert_eq!(stats.strings, 1);
    }
}

#[test]
fn every_c1_control_is_dropped_by_byte() {
    for c1 in 0x80u8..=0x9f {
        assert_eq!(once(&[b'a', c1, b'b']), b"ab", "{c1:#x}");
    }
}

#[test]
fn every_c1_control_is_dropped_by_code_point() {
    for c1 in 0x80u8..=0x9f {
        assert_eq!(once(&[b'a', 0xc2, c1, b'b']), b"ab", "U+00{c1:02x}");
    }
}

#[test]
fn a_byte_in_the_c1_range_that_continues_a_utf8_sequence_is_text() {
    // U+20AC is E2 82 AC; 82 is in the C1 range as a byte.
    assert_eq!(once(b"\xe2\x82\xac"), b"\xe2\x82\xac");
    // U+00A0 and U+00FF are just outside the C1 code points.
    assert_eq!(once(b"\xc2\xa0\xc3\xbf"), b"\xc2\xa0\xc3\xbf");
}

#[test]
fn invalid_utf8_is_dropped() {
    for bad in [
        &b"\xc0\x80"[..],
        b"\xc1\xbf",
        b"\xe0\x80\x80",
        b"\xed\xa0\x80",
        b"\xf0\x80\x80\x80",
        b"\xf4\x90\x80\x80",
        b"\xf5\x80\x80\x80",
        b"\xff",
        b"\xa9",
    ] {
        let mut s = b"a".to_vec();
        s.extend_from_slice(bad);
        s.push(b'b');
        let out = once(&s);
        assert!(std::str::from_utf8(&out).is_ok(), "{bad:?} -> {out:?}");
        assert!(!out.iter().any(|b| (0x80..=0x9f).contains(b)), "{bad:?}");
    }
    assert_eq!(once(b"a\xe2\x82b"), b"ab", "a cut sequence is dropped");
}

#[test]
fn osc_outside_the_allowlist_is_dropped_and_the_allowlist_passes() {
    for n in [
        "8;;http://x",
        "52;c;c2VjcmV0",
        "133;A",
        "7;file:///",
        "9;note",
        "1337;x",
    ] {
        let s = format!("a\x1b]{n}\x07b");
        assert_eq!(once(s.as_bytes()), b"ab", "{n}");
    }
    for n in [
        "0;title",
        "1;icon",
        "2;title",
        "4;1;rgb:00/00/00",
        "10;?",
        "19;?",
        "104",
        "110;?",
        "119",
    ] {
        let s = format!("a\x1b]{n}\x07b");
        assert_eq!(once(s.as_bytes()), s.as_bytes(), "{n}");
        let s = format!("a\x1b]{n}\x1b\\b");
        assert_eq!(once(s.as_bytes()), s.as_bytes(), "{n} with ST");
    }
}

#[test]
fn an_osc_number_with_leading_zeros_or_extra_digits_is_not_an_allowlisted_one() {
    for n in [
        "02;t", "00;t", "0002;t", "20;t", "3;t", "5;t", "12a;t", ";t", "x",
    ] {
        let s = format!("a\x1b]{n}\x07b");
        assert_eq!(once(s.as_bytes()), b"ab", "{n}");
    }
}

#[test]
fn the_only_terminator_is_esc_backslash_and_bel_for_an_allowlisted_osc() {
    // The 8-bit ST, CAN and SUB, and BEL in a dropped string, end nothing.
    for end in [&b"\x9c"[..], b"\xc2\x9c", b"\x18", b"\x1a", b"\x07"] {
        let mut s = b"a\x1bPpay".to_vec();
        s.extend_from_slice(end);
        s.extend_from_slice(b"STILL IN THE STRING\x1b\\b");
        assert_eq!(once(&s), b"ab", "{end:?}");
    }
    // ESC then something else inside a string does not end it either.
    assert_eq!(once(b"a\x1bPx\x1b[2Jy\x1b\\b"), b"ab");
    assert_eq!(once(b"a\x1bPx\x1b\x1b\\b"), b"ab");
}

#[test]
fn a_string_over_4_kib_is_discarded_to_the_terminator_with_one_notice() {
    let mut s = b"a\x1bP".to_vec();
    s.extend(std::iter::repeat_n(b'x', 8 * 1024));
    s.extend_from_slice(b"\x1b[2Jcleared?\x1b\\b");
    let (out, notes, stats) = run(&[&s]);
    assert_eq!(out, b"ab", "an 8 KiB DCS has no VT effects");
    assert_eq!(notes, [Note::StringTruncated]);
    assert_eq!(stats.truncated, 1);
}

#[test]
fn an_unterminated_string_discards_everything_after_it() {
    let mut s = b"a\x1bP".to_vec();
    s.extend(std::iter::repeat_n(b'x', 2 * STRING_CAP));
    s.extend_from_slice(b"\x1b[2Jtext after");
    let (out, notes, _) = run(&[&s]);
    assert_eq!(out, b"a");
    assert_eq!(notes, [Note::StringTruncated], "one notice, no second cap");
}

#[test]
fn an_oversized_osc_is_dropped_whole_and_reported() {
    let mut s = b"a\x1b]0;".to_vec();
    s.extend(std::iter::repeat_n(b't', 2 * STRING_CAP));
    s.extend_from_slice(b"\x07not a terminator now\x1b\\b");
    let (out, notes, _) = run(&[&s]);
    assert_eq!(out, b"ab");
    assert_eq!(notes, [Note::StringTruncated]);
}

#[test]
fn a_string_of_exactly_the_cap_is_kept_to_its_end_and_one_more_byte_is_truncated() {
    for (len, truncated) in [
        (STRING_CAP - 1, false),
        (STRING_CAP, false),
        (STRING_CAP + 1, true),
    ] {
        let mut s = b"a\x1bP".to_vec();
        s.extend(std::iter::repeat_n(b'x', len));
        s.extend_from_slice(b"\x1b\\b");
        let (out, notes, _) = run(&[&s]);
        assert_eq!(out, b"ab", "{len}");
        assert_eq!(notes.len(), usize::from(truncated), "{len}");
    }
}

#[test]
fn esc_then_a_c1_control_removes_both() {
    assert_eq!(once(b"a\x1b\x9bb"), b"ab");
    assert_eq!(once(b"a\x1b\xc2\x9bb"), b"ab", "by code point too");
    assert_eq!(
        once(b"a\x1b\xc2\xa9b"),
        b"a\x1b\xc2\xa9b",
        "ESC then text is left alone"
    );
}

#[test]
fn what_is_held_back_at_the_end_of_the_stream_is_dropped() {
    for tail in [&b"\x1b"[..], b"\x1b]0;ti", b"\x1bPabc", b"\xe2\x82"] {
        let mut s = b"a".to_vec();
        s.extend_from_slice(tail);
        assert_eq!(once(&s), b"a", "{tail:?}");
    }
}

#[test]
fn a_sequence_split_at_every_position_is_treated_as_if_it_came_whole() {
    let s: &[u8] =
        b"x\x1b[31m\xe2\x82\xac\x1bPdcs\x1b\\\x9d\xc2\x9b\x1b]2;t\x07\x1b]52;c;QQ==\x07\xc2\xa0y";
    let whole = run(&[s]);
    for i in 0..=s.len() {
        assert_eq!(run(&[&s[..i], &s[i..]]), whole, "split at {i}");
    }
    let bytes: Vec<&[u8]> = s.chunks(1).collect();
    assert_eq!(run(&bytes), whole, "one byte at a time");
}

#[test]
fn an_allowlisted_osc_at_the_cap_passes_and_one_byte_more_is_dropped_with_a_notice() {
    for (extra, ok) in [(0usize, true), (1, false)] {
        let head = b"0;";
        let mut s = b"a\x1b]".to_vec();
        s.extend_from_slice(head);
        s.extend(std::iter::repeat_n(b't', STRING_CAP - head.len() + extra));
        s.extend_from_slice(b"\x1b\\b");
        let (out, notes, _) = run(&[&s]);
        assert_eq!(out == s, ok, "extra {extra}");
        assert_eq!(notes.len(), usize::from(!ok), "extra {extra}");
        if !ok {
            assert_eq!(out, b"ab");
        }
    }
}
