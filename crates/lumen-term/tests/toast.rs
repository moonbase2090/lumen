//! Toasts in the raster: where they are drawn, and what they cannot reach.

use lumen_term::notify::{Desk, Notifier, Outcome, Place, Settings, Source, Toast};
use lumen_term::{Document, Frame, Rect, Renderer, Window, layout, notification_area};

const WIN: Window = Window {
    cols: 80,
    rows: 24,
    editor_row: 19,
    editor_rows: 2,
};

fn renderer() -> Renderer {
    Renderer::light(1).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

fn doc() -> Document {
    Document::new(76, 4)
}

/// Toasts as the notifier would show them: sealed notifications on layers that
/// each have their own spacing.
fn toasts(texts: &[&str], unsigned: bool) -> Vec<Toast> {
    let settings = Settings {
        allow_unsigned: true,
        ..Settings::default()
    };
    let mut n = Notifier::new(settings);
    n.set_desk(Desk {
        focused: true,
        muted: false,
    });
    let source = if unsigned {
        Source::UnsignedGrid
    } else {
        Source::Sealed
    };
    for (i, text) in texts.iter().enumerate() {
        let id = u32::try_from(i).unwrap_or(0) + 1;
        assert!(n.add_layer(id, &format!("tab {id}"), Place::Local));
        assert_eq!(n.notify(0, id, source, text), Outcome::Toast, "{text:?}");
    }
    n.shown().to_vec()
}

fn lay() -> lumen_term::Layout {
    layout(WIN.cols, WIN.rows, WIN.editor_row, WIN.editor_rows, None)
}

fn changed(r: &Renderer, a: &Frame, b: &Frame, rect: &Rect) -> usize {
    let (cw, ch) = r.cell();
    let mut count = 0;
    for y in rect.y * ch..(rect.y + rect.h) * ch {
        for x in rect.x * cw..(rect.x + rect.w) * cw {
            if a.pixel(x, y) != b.pixel(x, y) {
                count += 1;
            }
        }
    }
    count
}

fn everything() -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: WIN.cols,
        h: WIN.rows,
    }
}

/// The lowest row (in pixels) that differs, inside the whole frame.
fn last_changed_row(a: &Frame, b: &Frame) -> Option<u32> {
    (0..a.height)
        .rev()
        .find(|y| (0..a.width).any(|x| a.pixel(x, *y) != b.pixel(x, *y)))
}

#[test]
fn the_notification_area_is_inside_the_content_and_off_every_other_part() {
    for editor_row in [10, 15, 19, 21] {
        for strip in [None, Some((false, 2)), Some((true, 5))] {
            let l = layout(WIN.cols, WIN.rows, editor_row, 2, strip);
            let area = notification_area(&l);
            assert!(l.content.contains(&area), "{editor_row} {strip:?}");
            assert!(!area.overlaps(&l.editor));
            for m in &l.margin {
                assert!(!area.overlaps(m));
            }
            if let lumen_term::StripPlacement::Placed(s) = l.strip {
                assert!(!area.overlaps(&s));
            }
            assert!(area.w <= lumen_term::layout::TOAST_COLUMNS);
        }
    }
}

#[test]
fn a_toast_changes_pixels_in_the_notification_area_and_nowhere_else() {
    let r = renderer();
    let base = r.render_window(&doc(), &WIN);
    let shown = toasts(&["build done"], false);
    let f = r.render_window_with(&doc(), &WIN, &shown);
    let area = notification_area(&lay());
    let inside = changed(&r, &base, &f, &area);
    assert!(inside > 0, "the toast is drawn");
    assert_eq!(
        changed(&r, &base, &f, &everything()),
        inside,
        "nothing outside the area changed"
    );
    let l = lay();
    for m in &l.margin {
        assert_eq!(changed(&r, &base, &f, m), 0);
    }
    assert_eq!(changed(&r, &base, &f, &l.editor), 0);
}

#[test]
fn hostile_program_text_cannot_leave_its_box() {
    let r = renderer();
    let base = r.render_window(&doc(), &WIN);
    let area = notification_area(&lay());
    let long = "x".repeat(5000);
    let hostile = [
        "lumen lumen lumen lumen lumen lumen",
        long.as_str(),
        "\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}",
        "a\u{202e}b\u{200b}c \u{2714} exit 0 (verified)",
        "<b>bold</b> & more \u{1b}[31m",
        "line one\nline two\nline three\nline four",
    ];
    for text in hostile {
        for unsigned in [false, true] {
            let shown = toasts(&[text], unsigned);
            assert_eq!(shown.len(), 1, "{text:?}");
            let f = r.render_window_with(&doc(), &WIN, &shown);
            let total = changed(&r, &base, &f, &everything());
            assert_eq!(
                total,
                changed(&r, &base, &f, &area),
                "{text:?} unsigned={unsigned}: spilled"
            );
            let l = lay();
            for m in &l.margin {
                assert_eq!(changed(&r, &base, &f, m), 0, "{text:?}: margin");
            }
            assert_eq!(changed(&r, &base, &f, &l.editor), 0, "{text:?}: editor");
        }
    }
}

#[test]
fn an_unsigned_toast_carries_the_unverified_output_label_as_an_extra_row() {
    let r = renderer();
    let base = r.render_window(&doc(), &WIN);
    let (_, cell_h) = r.cell();
    let signed = r.render_window_with(&doc(), &WIN, &toasts(&["done"], false));
    let unsigned = r.render_window_with(&doc(), &WIN, &toasts(&["done"], true));
    let a = last_changed_row(&base, &signed).unwrap_or(0);
    let b = last_changed_row(&base, &unsigned).unwrap_or(0);
    assert_eq!(b - a, cell_h, "one row taller");
    // The extra row holds the label, in the label colour; a signed toast has none.
    let label = r.theme().label;
    let area = notification_area(&lay());
    let (cw, ch) = r.cell();
    let first_row = |f: &Frame| {
        (area.y * ch..area.y * ch + ch)
            .any(|y| (area.x * cw..(area.x + area.w) * cw).any(|x| f.pixel(x, y) == Some(label)))
    };
    assert!(first_row(&unsigned), "the unverified-output label is drawn");
    assert!(!first_row(&signed), "a signed toast has no such label");
}

#[test]
fn sixteen_toasts_draw_only_as_many_as_fit_and_nothing_below_the_area() {
    let r = renderer();
    let base = r.render_window(&doc(), &WIN);
    let texts: Vec<String> = (0..16)
        .map(|i| format!("message number {i} with some longer text"))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let shown = toasts(&refs, false);
    assert_eq!(shown.len(), 16);
    let f = r.render_window_with(&doc(), &WIN, &shown);
    let area = notification_area(&lay());
    let (_, ch) = r.cell();
    let bottom = (area.y + area.h) * ch;
    let last = last_changed_row(&base, &f).unwrap_or(0);
    assert!(
        last + 1 < bottom,
        "drawn down to {last}, the area ends at {bottom}: a toast that does not fit must not be cut off at the edge"
    );
    assert_eq!(
        changed(&r, &base, &f, &everything()),
        changed(&r, &base, &f, &area)
    );
    // More fit than are drawn in a smaller window: drawing fewer changes fewer pixels.
    let few = r.render_window_with(&doc(), &WIN, &shown[..2]);
    assert!(changed(&r, &base, &f, &area) > changed(&r, &base, &few, &area));
}

#[test]
fn toasts_stack_from_the_top_of_the_area_in_order() {
    let r = renderer();
    let base = r.render_window(&doc(), &WIN);
    let area = notification_area(&lay());
    let (cw, ch) = r.cell();
    let one = r.render_window_with(&doc(), &WIN, &toasts(&["first"], false));
    let top_row =
        (0..one.height).find(|y| (0..one.width).any(|x| one.pixel(x, *y) != base.pixel(x, *y)));
    assert_eq!(
        top_row,
        Some(area.y * ch),
        "the first toast starts at the top of the area"
    );
    let left_col =
        (0..one.width).find(|x| (0..one.height).any(|y| one.pixel(*x, y) != base.pixel(*x, y)));
    assert_eq!(
        left_col,
        Some(area.x * cw),
        "and fills the width of the area"
    );
}

#[test]
fn no_toast_means_the_same_pixels_as_before() {
    let r = renderer();
    assert_eq!(
        r.render_window(&doc(), &WIN),
        r.render_window_with(&doc(), &WIN, &[])
    );
}

#[test]
fn wide_characters_in_a_narrow_window_neither_panic_nor_leave_the_area() {
    let r = renderer();
    // Two-cell characters, three bytes each, in text that must wrap at every width.
    let cjk = "\u{4e2d}\u{6587}\u{4e2d}\u{6587} \u{4e2d}\u{6587}\u{6587}";
    for cols in [4, 5, 6, 7, 8, 9, 12, 80] {
        let win = Window {
            cols,
            rows: 24,
            editor_row: 19,
            editor_rows: 2,
        };
        let empty = Document::new(
            usize::try_from(cols.saturating_sub(4)).unwrap_or(0).max(1),
            4,
        );
        let base = r.render_window(&empty, &win);
        for unsigned in [false, true] {
            let shown = toasts(&[cjk, "\u{4e2d}"], unsigned);
            let f = r.render_window_with(&empty, &win, &shown);
            let l = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
            let area = notification_area(&l);
            let all = Rect {
                x: 0,
                y: 0,
                w: win.cols,
                h: win.rows,
            };
            assert_eq!(
                changed(&r, &base, &f, &all),
                changed(&r, &base, &f, &area),
                "cols {cols} unsigned={unsigned}: spilled"
            );
        }
    }
}

#[test]
fn a_wide_character_in_a_narrow_rich_block_does_not_panic_either() {
    use lumen_term::{Context, Passthrough, Piece};
    let r = renderer();
    for columns in [1usize, 2, 3, 4, 5, 6] {
        let mut d = Document::new(columns, 2);
        d.receive(&mut Passthrough, b"x", Context::default());
        d.apply(vec![Piece::Rich(lumen_track0::Block::Paragraph(
            "\u{4e2d}\u{6587}\u{4e2d}\u{6587} \u{4e2d}".to_owned(),
        ))]);
        let _ = r.render(&d);
    }
}
