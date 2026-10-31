//! What a paste needs before it reaches a program.

#![cfg(feature = "links-clipboard")]

use lumen_term::paste::{Decision, Hold, Pasted, Rejection, judge};

fn sent(text: &str, bracketed: bool) -> Pasted {
    match judge(text, bracketed) {
        Decision::Send(p) => p,
        other => unreachable!("{text:?} should be sent: {other:?}"),
    }
}

fn held(text: &str) -> Hold {
    match judge(text, false) {
        Decision::Hold(h) => h,
        other => unreachable!("{text:?} should be held: {other:?}"),
    }
}

#[test]
fn a_plain_single_line_paste_is_sent_as_it_is() {
    for bracketed in [true, false] {
        let p = sent("echo hello\tworld", bracketed);
        assert_eq!(p.text, "echo hello\tworld");
        assert_eq!((p.controls_removed, p.stripped), (0, 0));
    }
}

#[test]
fn escape_c0_del_and_c1_controls_and_the_end_marker_are_removed() {
    let p = sent(
        "a\u{1b}[31mb\u{7}c\u{0}d\u{7f}e\u{85}f\u{9b}g\u{1b}[201~h",
        true,
    );
    assert_eq!(p.text, "a[31mbcdefgh");
    // ESC (twice, the second inside the marker is removed with it), BEL, NUL, DEL, two C1, and the marker.
    assert_eq!(p.controls_removed, 7, "{p:?}");
    assert!(!p.text.contains('\u{1b}'));
}

#[test]
fn the_end_marker_cannot_close_the_bracket_early() {
    let p = sent("safe\u{1b}[201~; rm -rf ~", true);
    assert_eq!(p.text, "safe; rm -rf ~");
    // A marker split by a removable control is closed up by the removal and
    // must not reappear.
    let q = sent("\u{1b}[20\u{7}1~", true);
    assert!(!q.text.contains('\u{1b}'));
}

#[test]
fn a_bracketed_multi_line_paste_is_sent_with_its_newlines() {
    let p = sent("line one\nline two\n", true);
    assert_eq!(p.text, "line one\nline two\n");
}

#[test]
fn a_paste_with_a_frame_introducer_is_rejected_and_never_parsed() {
    for bracketed in [true, false] {
        assert_eq!(
            judge("before \u{1b}_\u{8a}LUMEN\u{1}\u{f3}\u{80}rest", bracketed),
            Decision::Reject(Rejection::FrameIntroducer)
        );
        assert_eq!(
            judge("\u{1b}_x", bracketed),
            Decision::Reject(Rejection::FrameIntroducer)
        );
    }
}

#[test]
fn pasted_command_text_loses_bidi_controls_and_invisible_characters_and_they_are_counted() {
    let p = sent("ls\u{202e} -la\u{200b}\u{2066}", true);
    assert_eq!(p.text, "ls -la");
    assert_eq!(p.stripped, 3);
}

#[test]
fn without_bracketed_paste_a_line_break_holds_the_paste() {
    for text in ["a\nb", "a\rb", "a\r\nb", "ls\n"] {
        let h = held(text);
        assert!(!h.preview.is_empty(), "{text:?}");
    }
}

#[test]
fn the_confirmation_shows_every_control_character_visibly() {
    let h = held("echo a\u{7}b\u{1b}[31m\nrm x\r\u{7f}");
    assert!(
        h.preview.contains("\u{27e8}U+0007\u{27e9}"),
        "{}",
        h.preview
    );
    assert!(
        h.preview.contains("\u{27e8}U+001B\u{27e9}"),
        "{}",
        h.preview
    );
    assert!(
        h.preview.contains("\u{27e8}U+000A\u{27e9}\n"),
        "{}",
        h.preview
    );
    assert!(
        h.preview.contains("\u{27e8}U+000D\u{27e9}"),
        "{}",
        h.preview
    );
    assert!(
        h.preview.contains("\u{27e8}U+007F\u{27e9}"),
        "{}",
        h.preview
    );
    assert!(
        !h.preview
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "no raw control survives in the preview"
    );
}

#[test]
fn the_user_may_paste_it_as_a_single_line_instead() {
    let h = held("first\nsecond\r\nthird\rfourth");
    assert_eq!(h.single_line.text, "first second third fourth");
}

#[test]
fn paste_as_written_keeps_the_line_breaks_and_other_controls_but_never_escape_or_c1() {
    let h = held("a\u{7}b\nc\u{1b}[31m\u{85}d");
    assert_eq!(h.as_written.text, "a\u{7}b\nc[31md");
    assert!(!h.as_written.text.contains('\u{1b}'));
    assert!(!h.as_written.text.contains('\u{85}'));
    // The default choices drop the other controls.
    assert_eq!(h.single_line.text, "ab c[31md");
}

#[test]
fn without_bracketed_paste_another_control_alone_also_holds_the_paste() {
    let h = held("rm\u{7} -rf");
    assert!(h.preview.contains("\u{27e8}U+0007\u{27e9}"));
    assert_eq!(h.single_line.text, "rm -rf");
}

#[test]
fn the_held_choices_are_stripped_of_bidi_and_invisible_characters_too() {
    let h = held("ls\u{202e}\nrm\u{200b}");
    assert_eq!(h.single_line.text, "ls rm");
    assert_eq!(h.as_written.text, "ls\nrm");
}

#[test]
fn the_confirmation_cannot_reorder_or_hide_text_with_bidi_or_invisible_characters() {
    let text = "echo a\u{202e}b\u{2066}c\u{200f}d\u{200b}e\u{200d}f\u{feff}g\u{e0041}h\u{61c}i\n";
    let h = held(text);
    for escape in [
        "\u{27e8}U+202E\u{27e9}",
        "\u{27e8}U+2066\u{27e9}",
        "\u{27e8}U+200F\u{27e9}",
        "\u{27e8}U+200B\u{27e9}",
        "\u{27e8}U+200D\u{27e9}",
        "\u{27e8}U+FEFF\u{27e9}",
        "\u{27e8}U+E0041\u{27e9}",
        "\u{27e8}U+061C\u{27e9}",
    ] {
        assert!(h.preview.contains(escape), "{escape} in {}", h.preview);
    }
    assert!(
        !h.preview
            .chars()
            .any(|c| lumen_track0::is_bidi(c) || lumen_track0::is_default_ignorable(c)),
        "no bidi or invisible code point survives raw in the preview: {}",
        h.preview
    );
    // The choices the user can make are still stripped and counted as before.
    assert_eq!(h.single_line.text, "echo abcdefghi ");
    assert!(h.single_line.stripped > 0);
}
