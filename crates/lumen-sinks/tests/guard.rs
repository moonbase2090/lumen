//! The notification guard on its own: what it removes and what it keeps.

use lumen_sinks::guard::Guard;

/// Filter `reads` in order and return the bytes passed on and the number found.
fn run(reads: &[&[u8]]) -> (Vec<u8>, usize) {
    let mut g = Guard::new();
    let (mut out, mut found) = (Vec::new(), Vec::new());
    for r in reads {
        g.filter(r, &mut out, &mut found);
    }
    g.finish(&mut out);
    (out, found.len())
}

#[test]
fn a_notification_is_removed_with_any_terminator() {
    for bytes in [
        &b"a\x1b]9;hi\x07b"[..],
        &b"a\x1b]9;hi\x1b\\b"[..],
        &b"a\x1b]9;hi\x9cb"[..],
        &b"a\x1b]777;notify;t;b\x07b"[..],
        &b"a\x1b]99;i=1;x\x07b"[..],
        &b"a\x1b]9\x07b"[..],
    ] {
        let (out, n) = run(&[bytes]);
        assert_eq!((out.as_slice(), n), (&b"ab"[..], 1), "{bytes:?}");
    }
}

#[test]
fn other_sequences_pass_on_byte_for_byte() {
    for bytes in [
        &b"plain text\r\n"[..],
        &b"\x1b[31mred\x1b[0m"[..],
        &b"\x1b]0;a title\x07"[..],
        &b"\x1b]52;c;ZXZpbA==\x07"[..],
        &b"\x1b]8;;http://example.invalid/\x1b\\link\x1b]8;;\x1b\\"[..],
        &b"\x1b]10;?\x07"[..],
        &b"\x1b]91;x\x07"[..],
        &b"\x1b]7770;x\x07"[..],
        &b"\x1b]123456789;x\x07"[..],
        &b"\xe2\x9f\x9d9;not an osc\x07"[..],
    ] {
        let (out, n) = run(&[bytes]);
        assert_eq!((out.as_slice(), n), (bytes, 0), "{bytes:?}");
    }
}

#[test]
fn a_split_at_every_position_gives_the_same_result() {
    let whole = b"x\x1b]9;hello\x1b\\y\x1b]0;t\x07z\x1b]777;notify;a;b\x07w";
    let (want, want_n) = run(&[whole]);
    assert_eq!((want.as_slice(), want_n), (&b"xy\x1b]0;t\x07zw"[..], 2));
    for i in 0..=whole.len() {
        let (out, n) = run(&[&whole[..i], &whole[i..]]);
        assert_eq!((out, n), (want.clone(), want_n), "split at {i}");
    }
    let bytes: Vec<&[u8]> = whole.chunks(1).collect();
    assert_eq!(run(&bytes), (want, want_n), "byte at a time");
}

#[test]
fn an_escape_inside_an_intercepted_body_starts_a_new_sequence() {
    // ESC [ is not ST: the notification ends there and the CSI is passed on.
    let (out, n) = run(&[b"\x1b]9;abc\x1b[31mred"]);
    assert_eq!((out.as_slice(), n), (&b"\x1b[31mred"[..], 1));
    // ESC ] inside the body starts the next OSC, which here is a title.
    let (out, n) = run(&[b"\x1b]9;abc\x1b]0;t\x07"]);
    assert_eq!((out.as_slice(), n), (&b"\x1b]0;t\x07"[..], 1));
    // CAN ends it.
    let (out, n) = run(&[b"\x1b]9;abc\x18after"]);
    assert_eq!((out.as_slice(), n), (&b"after"[..], 1));
}

#[test]
fn a_stream_that_ends_inside_an_introducer_passes_the_held_bytes_on() {
    assert_eq!(run(&[b"a\x1b"]), (b"a\x1b".to_vec(), 0));
    assert_eq!(run(&[b"a\x1b]12"]), (b"a\x1b]12".to_vec(), 0));
    // An unterminated notification is simply dropped.
    assert_eq!(run(&[b"a\x1b]9;never ends"]), (b"a".to_vec(), 1));
}

#[test]
fn an_unterminated_body_does_not_grow_memory() {
    let mut g = Guard::new();
    let (mut out, mut found) = (Vec::new(), Vec::new());
    g.filter(b"\x1b]9;", &mut out, &mut found);
    for _ in 0..1000 {
        g.filter(&[b'x'; 4096], &mut out, &mut found);
    }
    assert_eq!(out, Vec::<u8>::new());
    assert_eq!(found.len(), 1);
}
