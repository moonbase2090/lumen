//! Track 0 parser behaviour.

use lumen_track0::{
    Block, Error, KIND_CODE, KIND_HEADING, KIND_PARAGRAPH, MAX_ESCAPED, MAX_STRING, escaped_text,
    parse,
};

fn s(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(u16::MAX)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

#[test]
fn well_formed_blocks_parse() {
    assert_eq!(
        parse(KIND_PARAGRAPH, &s("hi")),
        Ok(Block::Paragraph("hi".into()))
    );
    let mut h = vec![2];
    h.extend(s("title"));
    assert_eq!(
        parse(KIND_HEADING, &h),
        Ok(Block::Heading {
            level: 2,
            text: "title".into()
        })
    );
    let mut c = s("rust");
    c.extend(s("fn main() {}"));
    assert_eq!(
        parse(KIND_CODE, &c),
        Ok(Block::Code {
            lang: "rust".into(),
            text: "fn main() {}".into()
        })
    );
}

#[test]
fn every_sealed_range_type_is_refused() {
    for kind in 0..0x80 {
        assert_eq!(
            parse(kind, &s("x")),
            Err(Error::SealedKind),
            "kind {kind:#x}"
        );
    }
}

#[test]
fn unknown_unsealed_types_are_refused() {
    for kind in 0x83..=0xff {
        assert_eq!(
            parse(kind, &s("x")),
            Err(Error::UnknownKind),
            "kind {kind:#x}"
        );
    }
}

#[test]
fn strings_over_the_cap_are_refused_and_the_cap_itself_is_allowed() {
    let at_cap = "a".repeat(MAX_STRING);
    assert!(parse(KIND_PARAGRAPH, &s(&at_cap)).is_ok());
    let over = "a".repeat(MAX_STRING + 1);
    assert_eq!(parse(KIND_PARAGRAPH, &s(&over)), Err(Error::StringTooLong));
    // A declared length over the cap is refused before any bytes are read.
    assert_eq!(
        parse(KIND_PARAGRAPH, &[0xff, 0xff]),
        Err(Error::StringTooLong)
    );
}

#[test]
fn truncated_trailing_and_invalid_payloads_are_refused() {
    assert_eq!(parse(KIND_PARAGRAPH, &[]), Err(Error::Malformed));
    assert_eq!(parse(KIND_PARAGRAPH, &[0, 5, b'a']), Err(Error::Malformed));
    let mut extra = s("ok");
    extra.push(0);
    assert_eq!(parse(KIND_PARAGRAPH, &extra), Err(Error::Malformed));
    assert_eq!(parse(KIND_PARAGRAPH, &[0, 1, 0xff]), Err(Error::NotUtf8));
    let mut bad = vec![0];
    bad.extend(s("t"));
    assert_eq!(parse(KIND_HEADING, &bad), Err(Error::BadLevel));
    bad[0] = 7;
    assert_eq!(parse(KIND_HEADING, &bad), Err(Error::BadLevel));
}

#[test]
fn no_input_makes_the_parser_panic() {
    // Every prefix of a valid frame and every single-byte mutation.
    let mut c = s("rust");
    c.extend(s("code"));
    for kind in 0..=255u8 {
        for end in 0..=c.len() {
            let _ = parse(kind, &c[..end]);
        }
        for i in 0..c.len() {
            for b in [0u8, 0x1b, 0x7f, 0xff] {
                let mut m = c.clone();
                m[i] = b;
                let _ = parse(kind, &m);
            }
        }
    }
}

#[test]
fn escaped_text_has_no_controls_and_is_capped() {
    let raw = b"\x1b]52;c;ZXZpbA==\x07\x00\xff\\ done";
    let text = escaped_text(raw);
    assert!(text.chars().all(|c| !c.is_control()), "{text:?}");
    assert!(text.starts_with('\u{241b}'));
    assert!(text.ends_with(&format!("[{} bytes]", raw.len())));
    let long = vec![b'a'; 10_000];
    let text = escaped_text(&long);
    assert_eq!(text.matches('a').count(), MAX_ESCAPED);
    assert!(text.ends_with("[10000 bytes]"));
}
