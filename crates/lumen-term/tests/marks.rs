//! The prompt marks a shell writes (OSC 133 A to D and OSC 7), noted as the
//! output arrives, across read boundaries and up to a limit.

use lumen_term::marks::{MARKS_KEPT, Mark, MarkScanner};

#[test]
fn prompt_marks_are_noted_in_order() {
    let mut scanner = MarkScanner::new();
    // One of them split across two reads, one ended by ESC backslash.
    for chunk in [
        &b"\x1b]7;file://host/a%20b\x07\x1b]133;A\x07$ \x1b]133;B\x07"[..],
        &b"ls\x1b]133;C\x07out\x1b]133;D;3"[..],
        &b"\x07\x1b]133;D\x07\x1b]133;Z\x07\x1b]133;A\x1b\\"[..],
    ] {
        scanner.feed(chunk);
    }
    assert_eq!(
        scanner.marks(),
        [
            Mark::Directory("file://host/a%20b".to_owned()),
            Mark::PromptStart,
            Mark::PromptEnd,
            Mark::CommandStart,
            Mark::CommandEnd(Some(3)),
            Mark::CommandEnd(None),
            Mark::PromptStart,
        ],
        "an unknown 133 sub-command is not a mark"
    );
}

#[test]
fn other_strings_and_text_are_not_marks() {
    let mut scanner = MarkScanner::new();
    scanner.feed(b"plain \x1b[31mred\x1b[0m \x1b]0;a title\x07 \x1b]8;;http://x\x07");
    assert_eq!(scanner.marks(), Vec::<Mark>::new());
}

#[test]
fn only_the_newest_marks_are_kept() {
    let mut scanner = MarkScanner::new();
    for _ in 0..(MARKS_KEPT + 10) {
        scanner.feed(b"\x1b]133;A\x07");
    }
    scanner.feed(b"\x1b]133;D;9\x07");
    let marks = scanner.marks();
    assert_eq!(marks.len(), MARKS_KEPT);
    assert_eq!(marks.last(), Some(&Mark::CommandEnd(Some(9))));
}

#[test]
fn a_string_that_never_ends_is_dropped_at_the_cap() {
    let mut scanner = MarkScanner::new();
    let mut long = b"\x1b]133;A".to_vec();
    long.extend(std::iter::repeat_n(b'x', 5000));
    scanner.feed(&long);
    scanner.feed(b"\x07\x1b]133;B\x07");
    assert_eq!(scanner.marks(), [Mark::PromptEnd]);
}
