//! What a copy carries: visible runs, code points, stripped controls, producers.

#![cfg(feature = "links-clipboard")]

use lumen_term::copy::{Kind, Run, copy, strip};

fn run(text: &str, kind: Kind, visible: bool, producer: u32) -> Run {
    Run {
        text: text.to_owned(),
        kind,
        visible,
        producer,
    }
}

#[test]
fn only_visible_runs_are_copied_and_hidden_ones_are_counted() {
    let out = copy(&[
        run("shown ", Kind::Text, true, 1),
        run("hidden", Kind::Text, false, 1),
        run("also", Kind::Text, true, 1),
        run("hidden too", Kind::Command, false, 2),
    ]);
    assert_eq!(out.text, "shown also");
    assert_eq!(out.hidden, 2);
    assert!(!out.spans_producers());
}

#[test]
fn paragraph_text_keeps_every_original_code_point() {
    let text = "a\u{202e}b\u{200b}c\u{200d}d\u{2066}e\u{feff}";
    let out = copy(&[run(text, Kind::Text, true, 1)]);
    assert_eq!(out.text, text);
    assert_eq!(out.stripped, 0);
}

#[test]
fn code_text_loses_bidi_controls_but_keeps_zero_width_characters() {
    let (text, stripped) = strip("a\u{202e}b\u{200b}c\u{2066}d\u{200f}e\u{61c}", Kind::Code);
    assert_eq!(text, "ab\u{200b}cde");
    assert_eq!(stripped, 4);
}

#[test]
fn command_text_loses_bidi_controls_and_invisible_characters_and_the_count_is_returned() {
    let input = "ls\u{202e}\u{200b}\u{200d}\u{2060}\u{feff}\u{e0041} -l\u{2069}";
    let out = copy(&[run(input, Kind::Command, true, 1)]);
    assert_eq!(out.text, "ls -l");
    assert_eq!(out.stripped, 7);
}

#[test]
fn the_stripped_ranges_are_exactly_the_specified_ones() {
    let in_range = |c: char, kind: Kind| strip(&c.to_string(), kind).1 == 1;
    for n in (0x202a..=0x202e)
        .chain(0x2066..=0x2069)
        .chain([0x200e, 0x200f, 0x61c])
    {
        let c = char::from_u32(n).unwrap_or('x');
        assert!(
            in_range(c, Kind::Code) && in_range(c, Kind::Command),
            "{n:#x}"
        );
    }
    for n in (0x200b..=0x200d)
        .chain(0x2060..=0x2064)
        .chain([0xfeff])
        .chain(0xe0000..=0xe007f)
    {
        let c = char::from_u32(n).unwrap_or('x');
        assert!(in_range(c, Kind::Command), "{n:#x}");
        assert!(!in_range(c, Kind::Code), "{n:#x} is not stripped from code");
    }
    // Neighbours of the ranges are kept.
    for n in [
        0x2029,
        0x202f,
        0x2065,
        0x206a,
        0x200a,
        0x2005,
        0xfeff - 1,
        0xe0080,
    ] {
        let c = char::from_u32(n).unwrap_or('x');
        assert!(!in_range(c, Kind::Command), "{n:#x}");
    }
}

#[test]
fn copy_is_code_points_in_logical_order_not_glyphs() {
    // Combining marks and a ZWJ sequence are copied as the code points they are.
    let text = "e\u{301} \u{1f468}\u{200d}\u{1f469} \u{5d0}\u{5d1}";
    let out = copy(&[run(text, Kind::Text, true, 1)]);
    assert_eq!(out.text, text);
}

#[test]
fn a_copy_that_spans_producers_keeps_the_split() {
    let out = copy(&[
        run("one ", Kind::Text, true, 1),
        run("two ", Kind::Text, true, 1),
        run("three", Kind::Command, true, 2),
        run(" four", Kind::Text, true, 1),
    ]);
    assert!(out.spans_producers());
    let parts: Vec<(u32, &str)> = out
        .parts
        .iter()
        .map(|p| (p.producer, p.text.as_str()))
        .collect();
    assert_eq!(parts, [(1, "one two "), (2, "three"), (1, " four")]);
    assert_eq!(out.text, "one two three four");
}

#[test]
fn a_hidden_run_between_two_runs_of_one_producer_does_not_split_them() {
    let out = copy(&[
        run("a", Kind::Text, true, 1),
        run("secret", Kind::Text, false, 9),
        run("b", Kind::Text, true, 1),
    ]);
    assert_eq!(out.parts.len(), 1);
    assert_eq!(out.text, "ab");
}
