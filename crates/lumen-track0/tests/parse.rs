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
            text: "fn main() {}".into(),
            stripped: 0
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
    // Worst case: every input byte expands to a four-byte escape.
    let controls = vec![0x01u8; 256];
    let text = escaped_text(&controls);
    let body = text.split(" [").next().unwrap_or("");
    assert!(body.len() <= MAX_ESCAPED, "{} bytes", body.len());
    assert!(text.ends_with("[256 bytes]"));
    // ESC input is three output bytes each and is capped the same way.
    let escs = vec![0x1bu8; 1000];
    let body = escaped_text(&escs);
    let body = body.split(" [").next().unwrap_or("");
    assert!(body.len() <= MAX_ESCAPED);
}

#[test]
fn control_characters_are_refused_in_every_string() {
    for c in [
        '\u{1b}', '\u{07}', '\u{00}', '\u{7f}', '\u{85}', '\u{9b}', '\r',
    ] {
        let text = format!("a{c}b");
        assert_eq!(
            parse(KIND_PARAGRAPH, &s(&text)),
            Err(Error::Control),
            "{c:?}"
        );
        let mut h = vec![1];
        h.extend(s(&text));
        assert_eq!(parse(KIND_HEADING, &h), Err(Error::Control));
        let mut code = s(&text);
        code.extend(s("x"));
        assert_eq!(parse(KIND_CODE, &code), Err(Error::Control));
        let mut code = s("x");
        code.extend(s(&text));
        assert_eq!(parse(KIND_CODE, &code), Err(Error::Control));
    }
    assert!(parse(KIND_PARAGRAPH, &s("line one\n\tline two")).is_ok());
}

#[cfg(feature = "zero-width-text")]
#[test]
fn bidi_controls_are_stripped_and_counted_in_code_and_kept_and_closed_elsewhere() {
    let mut code = s("");
    code.extend(s("a\u{202e}b\u{2066}c\u{200f}"));
    assert_eq!(
        parse(KIND_CODE, &code),
        Ok(Block::Code {
            lang: String::new(),
            text: "abc".into(),
            stripped: 3
        })
    );
    assert_eq!(
        parse(KIND_PARAGRAPH, &s("a\u{202e}b")),
        Ok(Block::Paragraph("a\u{202e}b\u{202c}".into())),
        "the control is kept and closed at the end of the block"
    );
    let mut h = vec![1];
    h.extend(s("\u{061c}x"));
    assert_eq!(
        parse(KIND_HEADING, &h),
        Ok(Block::Heading {
            level: 1,
            text: "\u{061c}x".into()
        })
    );
}

#[cfg(not(feature = "zero-width-text"))]
#[test]
fn bidi_controls_are_stripped_and_counted_in_code_and_visible_elsewhere() {
    let mut code = s("");
    code.extend(s("a\u{202e}b\u{2066}c\u{200f}"));
    assert_eq!(
        parse(KIND_CODE, &code),
        Ok(Block::Code {
            lang: String::new(),
            text: "abc".into(),
            stripped: 3
        })
    );
    assert_eq!(
        parse(KIND_PARAGRAPH, &s("a\u{202e}b")),
        Ok(Block::Paragraph("a\\u{202e}b".into()))
    );
    let mut h = vec![1];
    h.extend(s("\u{061c}x"));
    assert_eq!(
        parse(KIND_HEADING, &h),
        Ok(Block::Heading {
            level: 1,
            text: "\\u{061c}x".into()
        })
    );
}

// ---- zero-width and bidi characters in paragraph and heading text ----

#[cfg(feature = "zero-width-text")]
fn paragraph(text: &str) -> String {
    match parse(KIND_PARAGRAPH, &s(text)) {
        Ok(Block::Paragraph(t)) => t,
        other => unreachable!("a paragraph parses: {other:?}"),
    }
}

#[cfg(feature = "zero-width-text")]
fn heading(text: &str) -> String {
    let mut h = vec![1];
    h.extend(s(text));
    match parse(KIND_HEADING, &h) {
        Ok(Block::Heading { text, .. }) => text,
        other => unreachable!("a heading parses: {other:?}"),
    }
}

#[cfg(feature = "zero-width-text")]
#[test]
fn an_emoji_zwj_sequence_arrives_whole() {
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    assert_eq!(
        paragraph(family),
        family,
        "stripping the joiners would break the sequence"
    );
    assert_eq!(
        paragraph(&format!("see {family} and \u{2764}\u{fe0f}")),
        format!("see {family} and \u{2764}\u{fe0f}")
    );
    assert_eq!(heading(family), family);
    // A keycap and a skin tone: variation selector and modifier stay too.
    assert_eq!(
        paragraph("1\u{fe0f}\u{20e3} \u{1f44d}\u{1f3fd}"),
        "1\u{fe0f}\u{20e3} \u{1f44d}\u{1f3fd}"
    );
}

#[cfg(feature = "zero-width-text")]
#[test]
fn an_arabic_zwnj_word_arrives_whole() {
    let word = "\u{645}\u{6cc}\u{200c}\u{62e}\u{648}\u{627}\u{647}\u{645}";
    assert_eq!(
        paragraph(word),
        word,
        "the non-joiner decides how the letters connect"
    );
    // Indic conjuncts use the joiner and the non-joiner too.
    let indic = "\u{915}\u{94d}\u{200d}\u{937}";
    assert_eq!(paragraph(indic), indic);
}

#[cfg(feature = "zero-width-text")]
#[test]
fn zero_width_space_and_word_joiner_are_kept_in_the_text() {
    assert_eq!(
        paragraph("a\u{200b}b\u{2060}c\u{feff}d"),
        "a\u{200b}b\u{2060}c\u{feff}d"
    );
}

#[cfg(feature = "zero-width-text")]
#[test]
fn a_right_to_left_override_cannot_leak_past_its_line_or_block() {
    assert_eq!(
        paragraph("a\u{202e}b"),
        "a\u{202e}b\u{202c}",
        "closed at the end of the block"
    );
    assert_eq!(
        paragraph("a\u{202e}b\nc"),
        "a\u{202e}b\u{202c}\nc",
        "closed at the end of the line, so the next line is untouched"
    );
    assert_eq!(heading("\u{202e}x"), "\u{202e}x\u{202c}");
    // Nested and mixed: closed innermost first, each with its own closer.
    assert_eq!(
        paragraph("\u{202b}a\u{2067}b\u{202d}c"),
        "\u{202b}a\u{2067}b\u{202d}c\u{202c}\u{2069}\u{202c}"
    );
    // A closer with nothing open is left alone and opens nothing.
    assert_eq!(paragraph("a\u{202c}b\u{2069}c"), "a\u{202c}b\u{2069}c");
    // An isolate close ends the embeddings inside it.
    assert_eq!(
        paragraph("\u{2066}a\u{202e}b\u{2069}c"),
        "\u{2066}a\u{202e}b\u{2069}c",
        "the isolate's close also ends the override inside it, so nothing more to close"
    );
    // Marks are single characters and need no closer.
    assert_eq!(
        paragraph("a\u{200f}b\u{200e}c\u{61c}"),
        "a\u{200f}b\u{200e}c\u{61c}"
    );
}

/// Every bidi control removed: what is left is only the text.
#[cfg(feature = "zero-width-text")]
fn without_bidi(text: &str) -> String {
    text.chars()
        .filter(|c| !lumen_track0::is_bidi(*c))
        .collect()
}

/// The directional state at the end of each line: how many embeddings and
/// isolates are open, by the rules of UAX #9 for the controls used here.
#[cfg(feature = "zero-width-text")]
fn open_at_line_ends(text: &str) -> Vec<usize> {
    let mut stack: Vec<bool> = Vec::new();
    let mut ends = Vec::new();
    for c in text.chars() {
        match c {
            '\u{202a}'..='\u{202b}' | '\u{202d}'..='\u{202e}' => stack.push(false),
            '\u{2066}'..='\u{2068}' => stack.push(true),
            '\u{202c}' if stack.last() == Some(&false) => {
                stack.pop();
            }
            '\u{2069}' if stack.contains(&true) => {
                while let Some(isolate) = stack.pop() {
                    if isolate {
                        break;
                    }
                }
            }
            '\n' => ends.push(stack.len()),
            _ => {}
        }
    }
    ends.push(stack.len());
    ends
}

#[cfg(feature = "zero-width-text")]
#[test]
fn for_any_text_nothing_is_lost_and_nothing_is_left_open_at_a_line_end() {
    let alphabet: Vec<char> = "ab \n\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}\u{200e}\u{200f}\u{200b}\u{200d}\u{200c}\u{2060}\u{fe0f}\u{1f468}\u{645}"
        .chars()
        .collect();
    let mut state = 0x1234_5678_u64;
    for _ in 0..3000 {
        let mut text = String::new();
        for _ in 0..24 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            text.push(alphabet[(state >> 33) as usize % alphabet.len()]);
        }
        let out = paragraph(&text);
        // Nothing but bidi closers added, nothing removed.
        assert_eq!(without_bidi(&out), without_bidi(&text), "{text:?}");
        assert!(
            out.chars()
                .filter(|c| !lumen_track0::is_bidi(*c))
                .eq(text.chars().filter(|c| !lumen_track0::is_bidi(*c)))
        );
        // Every line ends with no control open.
        assert!(
            open_at_line_ends(&out).iter().all(|n| *n == 0),
            "{text:?} -> {out:?}"
        );
        // Idempotent: closing what is closed changes nothing.
        assert_eq!(paragraph(&out), out, "{text:?}");
    }
}

#[test]
fn a_link_label_or_target_shows_invisible_characters_as_escapes() {
    use lumen_track0::escape_invisible;
    assert_eq!(
        escape_invisible("pay\u{200b}pal.com"),
        "pay\u{27e8}U+200B\u{27e9}pal.com"
    );
    assert_eq!(
        escape_invisible("a\u{202e}b\u{200d}c\u{fe0f}\u{e0041}"),
        "a\u{27e8}U+202E\u{27e9}b\u{27e8}U+200D\u{27e9}c\u{27e8}U+FE0F\u{27e9}\u{27e8}U+E0041\u{27e9}"
    );
    assert_eq!(
        escape_invisible("plain text \u{4e2d}\u{6587}"),
        "plain text \u{4e2d}\u{6587}"
    );
    // Every default-ignorable code point in the table is escaped, none is lost.
    for cp in [
        0xad_u32, 0x34f, 0x61c, 0x115f, 0x1160, 0x17b4, 0x180b, 0x200b, 0x200f, 0x2060, 0x206f,
        0x3164, 0xfe0f, 0xfeff, 0xffa0, 0xfff8, 0x1bca0, 0x1d173, 0xe0001, 0xe0fff,
    ] {
        let c = char::from_u32(cp).unwrap_or('?');
        assert_eq!(
            escape_invisible(&c.to_string()),
            format!("\u{27e8}U+{cp:04X}\u{27e9}"),
            "{cp:x}"
        );
    }
}
