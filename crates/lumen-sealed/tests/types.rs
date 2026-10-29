//! The frame type table: capability bytes and payload parsing.

use lumen_sealed::types::{
    CAP_CLIPBOARD_WRITE, CAP_NONE, CAP_NOTIFICATIONS, CAP_SIDE_CHANNEL, Frame, KIND_COMMAND_END,
    KIND_COMMAND_START, KIND_GRANT, KIND_LAYOUT, KIND_MARKER, KIND_NOTIFICATION, KIND_PROMPT,
    Malformed, capability_of, parse,
};

const KNOWN: [u8; 9] = [0x01, 0x02, 0x10, 0x11, 0x12, 0x20, 0x21, 0x22, 0x30];

#[test]
fn the_capability_byte_is_fixed_by_the_type() {
    for kind in [0x01, 0x02, 0x10, 0x11, 0x12, 0x20] {
        assert_eq!(capability_of(kind), Some(CAP_NONE), "{kind:#x}");
    }
    assert_eq!(capability_of(0x21), Some(CAP_CLIPBOARD_WRITE));
    assert_eq!(capability_of(0x22), Some(CAP_NOTIFICATIONS));
    assert_eq!(capability_of(0x30), Some(CAP_SIDE_CHANNEL));
    for kind in (0..=255u8).filter(|k| !KNOWN.contains(k)) {
        assert_eq!(capability_of(kind), None, "{kind:#x}");
        assert_eq!(parse(kind, &[]), Err(Malformed::UnknownKind), "{kind:#x}");
    }
}

#[test]
fn well_formed_payloads_parse() {
    assert_eq!(
        parse(KIND_COMMAND_START, &[0, 0, 0, 7]),
        Ok(Frame::CommandStart { id: 7 })
    );
    assert_eq!(parse(KIND_PROMPT, &[]), Ok(Frame::Prompt));
    assert_eq!(
        parse(KIND_COMMAND_END, &[0, 0, 0, 7, 0xff, 0xff, 0xff, 0xff]),
        Ok(Frame::CommandEnd { id: 7, status: -1 })
    );
    assert_eq!(
        parse(KIND_GRANT, &[1, 7]),
        Ok(Frame::Grant {
            grant: true,
            capability: 7
        })
    );
    assert_eq!(
        parse(KIND_GRANT, &[0, 8]),
        Ok(Frame::Grant {
            grant: false,
            capability: 8
        })
    );
    assert_eq!(
        parse(0x21, b"copy me"),
        Ok(Frame::ClipboardWrite("copy me".into()))
    );
    assert_eq!(
        parse(KIND_NOTIFICATION, &[0, 2, b'h', b'i']),
        Ok(Frame::Notification("hi".into()))
    );
    let mut marker = vec![0, 0, 0, 3];
    marker.extend_from_slice(&[9; 32]);
    assert_eq!(
        parse(KIND_MARKER, &marker),
        Ok(Frame::Marker {
            id: 3,
            hash: [9; 32]
        })
    );
    assert_eq!(
        parse(KIND_LAYOUT, &[1, 2, 3]),
        Ok(Frame::Layout(vec![1, 2, 3]))
    );
}

#[test]
fn malformed_payloads_are_refused_with_the_reason() {
    assert_eq!(
        parse(KIND_COMMAND_START, &[0, 0, 7]),
        Err(Malformed::Length)
    );
    assert_eq!(parse(KIND_PROMPT, &[0]), Err(Malformed::Length));
    assert_eq!(parse(KIND_COMMAND_END, &[0; 7]), Err(Malformed::Length));
    assert_eq!(parse(KIND_GRANT, &[2, 1]), Err(Malformed::Field));
    assert_eq!(parse(KIND_GRANT, &[1, 0]), Err(Malformed::Field));
    assert_eq!(parse(KIND_GRANT, &[1, 9]), Err(Malformed::Field));
    assert_eq!(parse(KIND_GRANT, &[1]), Err(Malformed::Length));
    assert_eq!(parse(0x21, &[0xff, 0xfe]), Err(Malformed::Text));
    assert_eq!(parse(0x21, &vec![b'a'; 65537]), Err(Malformed::Text));
    assert!(parse(0x21, &vec![b'a'; 65536]).is_ok());
    assert_eq!(
        parse(KIND_NOTIFICATION, &[0, 5, b'h']),
        Err(Malformed::Length)
    );
    assert_eq!(parse(KIND_NOTIFICATION, &[0]), Err(Malformed::Length));
    assert_eq!(
        parse(KIND_NOTIFICATION, &[0, 1, 0xff]),
        Err(Malformed::Text)
    );
    let mut long = vec![0x10, 0x01];
    long.extend(vec![b'a'; 4097]);
    assert_eq!(parse(KIND_NOTIFICATION, &long), Err(Malformed::Text));
    assert_eq!(parse(KIND_MARKER, &[0; 35]), Err(Malformed::Length));
}

#[test]
fn no_payload_makes_the_parser_panic() {
    for kind in 0..=255u8 {
        for len in 0..80usize {
            for fill in [0u8, 1, 0x7f, 0xff] {
                let _ = parse(kind, &vec![fill; len]);
            }
        }
    }
}
