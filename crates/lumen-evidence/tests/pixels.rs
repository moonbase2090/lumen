//! Pixel mode: the oracle's own checks, then the committed corpus.

// Short names for frames, windows and rectangles keep the comparisons readable.
#![allow(clippy::many_single_char_names)]

use lumen_evidence::pixels::{PixelReport, changed_in, rects_text, run_pixels, run_pixels_with};
use lumen_evidence::{Corpus, Renderer, load_corpus};
use lumen_term::{Block, Document, Frame, Window, layout};
use std::cell::Cell;
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-pixel")
}

fn corpus() -> Corpus {
    load_corpus(&corpus_dir()).unwrap_or_else(|e| unreachable!("the pixel corpus loads: {e}"))
}

fn renderer() -> Renderer {
    Renderer::light(2).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

#[test]
fn changed_pixels_are_counted_once_even_where_two_rectangles_overlap() {
    let r = renderer();
    let w = Window {
        cols: 40,
        rows: 12,
        editor_row: 8,
        editor_rows: 2,
    };
    let empty = Document::new(1, 1);
    let a = r.render_window(&empty, &w);
    let mut b = a.clone();
    // One pixel in the corner where the gutter and the header strip overlap.
    let i = 0;
    b.rgba[i] ^= 0xff;
    let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
    assert_eq!(
        changed_in(&r, &a, &b, &l.margin),
        1,
        "the corner is in both rectangles"
    );
    assert_eq!(changed_in(&r, &a, &b, &[l.editor]), 0);
    assert_eq!(changed_in(&r, &a, &a, &l.margin), 0);
}

#[test]
fn the_committed_corpus_measures_zero_in_the_margin_and_the_editor() {
    let c = corpus();
    assert_eq!(c.revision, "px-1");
    let Ok((report, header)) = run_pixels(&c, "testsha", 2, None, Vec::new()) else {
        unreachable!("the run succeeds")
    };
    assert!(report.baseline_stable);
    assert_eq!(report.cases.len(), c.cases.len() * 2, "two windows");
    assert_eq!((report.margin(), report.chrome()), (0, 0));
    assert_eq!(report.exit_code(), 0);
    let s = report.summary(&header, &rects_text(&renderer()));
    assert!(s.contains("2.3 changed_pixels margin=0 chrome=0"), "{s}");
    assert!(s.contains("not an operating-system window"), "{s}");
}

/// Draws the window, then lets the first foreground block's words into the
/// header strip, as a renderer with a leak would.
fn leaky(r: &Renderer, d: &Document, w: &Window) -> Frame {
    let mut f = r.render_window(d, w);
    if d.blocks().iter().any(|b| matches!(b, Block::Rich(_))) {
        // Paint a few pixels where the header label is.
        for x in 200usize..260 {
            f.rgba[x * 4] ^= 0xff;
        }
    }
    f
}

#[test]
fn negative_control_a_leak_into_the_margin_is_counted_and_fails_the_run() {
    let c = corpus();
    let Ok((report, _)) = run_pixels_with(&c, "testsha", 2, None, Vec::new(), &leaky) else {
        unreachable!("the run succeeds")
    };
    assert!(report.baseline_stable);
    assert!(report.margin() > 0, "the oracle sees the leak");
    assert_eq!(report.exit_code(), 1);
    // The cases that carry no rich block are untouched.
    let clean = report
        .cases
        .iter()
        .find(|c| c.id == "scroll-flood-2000-lines");
    assert_eq!(clean.map(|c| c.margin), Some(0));
}

#[test]
fn negative_control_a_leak_into_the_editor_is_counted_too() {
    let c = corpus();
    let leak = |r: &Renderer, d: &Document, w: &Window| {
        let mut f = r.render_window(d, w);
        if d.blocks().iter().any(|b| matches!(b, Block::Rich(_))) {
            let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
            let (cw, ch) = r.cell();
            let idx = ((l.editor.y * ch + 5) * f.width + l.editor.x * cw + 40) * 4;
            f.rgba[idx as usize] ^= 0xff;
        }
        f
    };
    let Ok((report, _)) = run_pixels_with(&c, "testsha", 2, None, Vec::new(), &leak) else {
        unreachable!("the run succeeds")
    };
    assert!(report.chrome() > 0 && report.margin() == 0);
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn an_unstable_baseline_makes_the_oracle_invalid_and_exits_3() {
    let c = corpus();
    let calls = Cell::new(0u32);
    let flaky = |r: &Renderer, d: &Document, w: &Window| {
        let mut f = r.render_window(d, w);
        let n = calls.get();
        calls.set(n + 1);
        if d.blocks().is_empty() && n % 2 == 1 {
            f.rgba[0] ^= 0xff;
        }
        f
    };
    let Ok((report, header)) = run_pixels_with(&c, "testsha", 2, None, Vec::new(), &flaky) else {
        unreachable!("the run succeeds")
    };
    assert!(!report.baseline_stable);
    assert_eq!(report.exit_code(), 3);
    let s = report.summary(&header, "");
    assert!(
        s.contains("2.3=not measured (the baseline taken twice did not match"),
        "{s}"
    );
    assert!(
        !s.contains("changed_pixels"),
        "no count is printed for an invalid oracle"
    );
}

#[test]
fn no_cases_is_not_measured_and_not_a_pass() {
    let report = PixelReport {
        scale: 2,
        cell: (17, 37),
        cases: Vec::new(),
        baseline_stable: true,
        baseline_digest: String::new(),
    };
    assert_eq!(report.exit_code(), 3);
}

#[test]
fn the_rectangles_are_committed_with_the_corpus_revision() {
    let committed = std::fs::read_to_string(corpus_dir().join("margin-rects.txt"));
    let want = format!("corpus px-1, scale 2\n{}", rects_text(&renderer()));
    if std::env::var_os("LUMEN_REGENERATE_PIXELS").is_some() {
        assert!(std::fs::write(corpus_dir().join("margin-rects.txt"), &want).is_ok());
    }
    let committed =
        committed.or_else(|_| std::fs::read_to_string(corpus_dir().join("margin-rects.txt")));
    assert_eq!(committed.ok(), Some(want));
}

#[test]
fn a_run_writes_the_baseline_and_a_png_for_every_case_and_window() {
    let c = corpus();
    let dir = std::env::temp_dir().join(format!("lumen-pixels-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let Ok((report, _)) = run_pixels(&c, "testsha", 1, Some(&dir), Vec::new()) else {
        unreachable!("the run succeeds")
    };
    let shots = dir.join("screenshots");
    let count = std::fs::read_dir(&shots).map_or(0, Iterator::count);
    assert_eq!(
        count,
        report.cases.len() + 2,
        "a PNG each plus the two baselines"
    );
    assert!(shots.join("baseline-80x24.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
