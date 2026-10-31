//! Gate G12, the trust margin: under adversarial Track 0 input, the pixels of
//! the trust margin stay unchanged ([`gate.trust-margin`]).
//!
//! The oracle is a pixel diff. A window is rendered with no producer input and
//! again with the input under test, and the gutter and the header strip must not
//! differ by one pixel. The editor is held to the same standard, because it is
//! the other terminal-drawn part a producer could try to move. The matrix is
//! window size by scale by container state by input, and the inputs include
//! every unsealed and sealed type byte, escape sequences that move the cursor or
//! change the screen, and seeded random bytes.
//!
//! A final test shows the oracle can fail: a renderer that lets content into the
//! margin is caught.

#![allow(clippy::many_single_char_names)]

use lumen_detector::TAG;
use lumen_term::notify::Toast;
use lumen_term::{Context, Document, Frame, Layer, Rect, Renderer, Window, layout};

/// The window sizes under test: usual, narrow, short, and one smaller than the
/// margin itself.
const WINDOWS: [Window; 4] = [
    Window {
        cols: 80,
        rows: 24,
        editor_row: 19,
        editor_rows: 2,
    },
    Window {
        cols: 30,
        rows: 12,
        editor_row: 8,
        editor_rows: 2,
    },
    Window {
        cols: 120,
        rows: 6,
        editor_row: 3,
        editor_rows: 1,
    },
    Window {
        cols: 3,
        rows: 2,
        editor_row: 1,
        editor_rows: 1,
    },
];

fn renderer(scale: u32) -> Renderer {
    Renderer::light(scale).unwrap_or_else(|e| unreachable!("the bundled face loads: {e}"))
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn paragraph(text: &str) -> Vec<u8> {
    frame(0x80, &string(text))
}

fn b_layer(container: bool) -> lumen_arm_b::Layer {
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    if container {
        Layer::open_container(&mut l);
    }
    l
}

/// A small deterministic generator, so a failure names a seed that reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next().to_be_bytes()[3]).collect()
    }
}

/// Pieces a hostile producer would try: lookalikes of the margin's own text, the
/// verified badges, every cursor and screen control, and oversize content.
fn adversarial() -> Vec<(String, Vec<u8>)> {
    let v: Vec<(String, Vec<u8>)> = vec![
        (
            "badges".into(),
            paragraph("\u{2714} verified \u{23fa} exit 0 [ok] exit status 0"),
        ),
        (
            "margin-text".into(),
            paragraph("unverified output lumen lumen lumen"),
        ),
        ("long-paragraph".into(), paragraph(&"w".repeat(4000))),
        ("many-paragraphs".into(), {
            (0..300).flat_map(|i| paragraph(&format!("row {i}"))).collect()
        }),
        (
            "bidi-and-zero-width".into(),
            paragraph("a\u{202e}b\u{200b}c\u{2066}d\u{200f}"),
        ),
        (
            "wide-and-combining".into(),
            paragraph("\u{4e2d}\u{6587}\u{4e2d} e\u{301}\u{301}\u{301}\u{301}\u{301}"),
        ),
        (
            "home-clear-write".into(),
            b"\x1b[H\x1b[2J\x1b[3Jlumen\x1b[1;1Hlumen\x1b[24;1Hlumen".to_vec(),
        ),
        (
            "absolute-cursor-everywhere".into(),
            (1..=30)
                .flat_map(|r| format!("\x1b[{r};1HX\x1b[{r};200HY").into_bytes())
                .collect(),
        ),
        (
            "scroll-region-and-insert".into(),
            b"\x1b[1;3r\x1b[10L\x1b[10M\x1b[5S\x1b[5T\x1b[r".to_vec(),
        ),
        (
            "alt-screen".into(),
            b"\x1b[?1049h\x1b[2Jfull screen\x1b[?1049l".to_vec(),
        ),
        (
            "window-ops".into(),
            b"\x1b[8;1;1t\x1b[8;200;400t\x1b[3;0;0t\x1b[22;0t\x1b[23;0t".to_vec(),
        ),
        (
            "title-and-colours".into(),
            b"\x1b]0;lumen verified\x07\x1b]2;title\x1b\\\x1b]4;1;rgb:00/ff/00\x07\x1b]10;#00ff00\x07\x1b]11;#ff0000\x07"
                .to_vec(),
        ),
        (
            "reverse-video-and-full-width-fill".into(),
            (0..40)
                .flat_map(|_| b"\x1b[7m                                                                                \x1b[0m\r\n".to_vec())
                .collect(),
        ),
        (
            "rejected-header-text".into(),
            b"\x1b_BADTAG00\x80\x00\x00\x00\x04\x00\x02hi".to_vec(),
        ),
        (
            "truncated-frame".into(),
            {
                let mut f = paragraph("cut");
                f.truncate(f.len() - 2);
                f
            },
        ),
        (
            "lying-length".into(),
            {
                let mut f = b"\x1b_".to_vec();
                f.extend_from_slice(&TAG);
                f.push(0x80);
                f.extend_from_slice(&u32::MAX.to_be_bytes());
                f.extend(string("x"));
                f
            },
        ),
    ];
    v
}

/// Every type byte, unsealed (0x80 and up) and sealed (below), with a payload
/// that is valid for no one of them.
fn every_kind() -> Vec<(String, Vec<u8>)> {
    (0..=255u8)
        .map(|kind| (format!("kind-{kind:#04x}"), frame(kind, &string("payload"))))
        .collect()
}

/// Seeded random streams: raw bytes, and random bytes behind a real frame header.
fn random_streams() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for seed in 1..=24u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let len = 64 + usize::try_from(rng.next() % 3000).unwrap_or(64);
        let raw = rng.bytes(len);
        out.push((format!("raw-{seed}"), raw.clone()));
        let kind = 0x80 | rng.bytes(1)[0];
        out.push((format!("framed-{seed}"), frame(kind, &raw)));
        let mut mixed = b"\x1b[H".to_vec();
        mixed.extend(raw);
        mixed.extend(paragraph("tail"));
        out.push((format!("mixed-{seed}"), mixed));
    }
    out
}

fn px(r: &Renderer, rect: &Rect) -> (u32, u32, u32, u32) {
    let (cw, ch) = r.cell();
    (rect.x * cw, rect.y * ch, rect.w * cw, rect.h * ch)
}

/// Pixels that differ between two frames inside `rect` (in cells).
fn changed(r: &Renderer, left: &Frame, right: &Frame, rect: &Rect) -> usize {
    let (x0, y0, w, h) = px(r, rect);
    let mut n = 0;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            if left.pixel(x, y) != right.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

fn doc_for(win: &Window) -> Document {
    let cols = usize::try_from(win.cols.saturating_sub(4))
        .unwrap_or(1)
        .max(1);
    let rows = usize::try_from(win.rows.saturating_sub(1))
        .unwrap_or(1)
        .max(1);
    Document::new(cols, rows)
}

/// Feed `bytes` to a fresh document and render it. The bytes arrive whole, and
/// again in uneven chunks, so a split cannot hide a difference.
fn render(r: &Renderer, win: &Window, bytes: &[u8], container: bool, chunked: bool) -> Frame {
    let mut d = doc_for(win);
    let mut l = b_layer(container);
    if chunked {
        let mut at = 0;
        let mut step = 1;
        while at < bytes.len() {
            let end = (at + step).min(bytes.len());
            d.receive(&mut l, &bytes[at..end], Context::default());
            at = end;
            step = step % 7 + 1;
        }
        d.finish(&mut l);
    } else {
        d.receive(&mut l, bytes, Context::default());
    }
    r.render_window(&d, win)
}

fn margin_and_editor(win: &Window) -> Vec<(&'static str, Rect)> {
    let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
    vec![
        ("gutter", lay.margin[0]),
        ("header", lay.margin[1]),
        ("editor", lay.editor),
    ]
}

/// Render each input in each way and count the pixels of the margin and editor
/// that differ from the baseline. Returns how many renders were checked.
fn check(scale: u32, windows: &[Window], inputs: &[(String, Vec<u8>)], chunks: &[bool]) -> usize {
    let r = renderer(scale);
    let mut runs = 0;
    for win in windows {
        let base = r.render_window(&doc_for(win), win);
        let parts = margin_and_editor(win);
        for (name, bytes) in inputs {
            for container in [true, false] {
                for &chunked in chunks {
                    let f = render(&r, win, bytes, container, chunked);
                    for (part, rect) in &parts {
                        assert_eq!(
                            changed(&r, &base, &f, rect),
                            0,
                            "{name}: scale {scale}, {}x{}, container {container}, \
                             chunked {chunked}: {part} changed",
                            win.cols,
                            win.rows
                        );
                    }
                    runs += 1;
                }
            }
        }
    }
    runs
}

#[test]
fn no_named_or_random_input_changes_a_margin_or_editor_pixel_in_any_window() {
    let named = adversarial();
    let mut all = named.clone();
    all.extend(random_streams());
    // Scale 1: every window, whole and in uneven chunks. Scale 2: the named
    // inputs only, which keeps the run short.
    let one = check(1, &WINDOWS, &all, &[false, true]);
    let two = check(2, &WINDOWS, &named, &[false, true]);
    assert_eq!(one, 4 * (16 + 72) * 4);
    assert_eq!(two, 4 * 16 * 4);
}

#[test]
fn no_frame_type_byte_changes_a_margin_or_editor_pixel() {
    let kinds = every_kind();
    assert_eq!(kinds.len(), 256);
    // The largest and the smallest window, with and without the container.
    let windows = [WINDOWS[0], WINDOWS[3]];
    assert_eq!(check(1, &windows, &kinds, &[false]), 2 * 256 * 2);
}

#[test]
fn the_frame_size_and_the_baseline_do_not_depend_on_the_input() {
    for scale in [1, 2] {
        let r = renderer(scale);
        let (cw, ch) = r.cell();
        for win in &WINDOWS {
            let base = r.render_window(&doc_for(win), win);
            assert_eq!(
                (base.width, base.height),
                ((win.cols * cw).max(1), (win.rows * ch).max(1))
            );
            for (name, bytes) in adversarial() {
                let f = render(&r, win, &bytes, true, false);
                assert_eq!(
                    (f.width, f.height),
                    (base.width, base.height),
                    "{name}: producer input must not resize the window"
                );
            }
        }
    }
}

#[test]
fn background_output_and_an_open_strip_leave_the_margin_and_editor_alone() {
    let r = renderer(1);
    let win = &WINDOWS[0];
    let parts = margin_and_editor(win);
    let base = r.render_window(&doc_for(win), win);
    // No container: Track 0 is background output, which goes to the strip.
    let mut bytes = Vec::new();
    for i in 0..50 {
        bytes.extend(paragraph(&format!("background {i}")));
    }
    let mut d = doc_for(win);
    let mut l = b_layer(false);
    d.receive(&mut l, &bytes, Context::default());
    let cycle = d.cycle();
    let collapsed = r.render_window(&d, win);
    assert!(d.user_opens_strip(cycle), "the strip opens on user action");
    let open = r.render_window(&d, win);
    assert_ne!(
        base, collapsed,
        "the strip is drawn, so the test is not vacuous"
    );
    for (part, rect) in &parts {
        assert_eq!(changed(&r, &base, &collapsed, rect), 0, "collapsed: {part}");
        assert_eq!(changed(&r, &base, &open, rect), 0, "open: {part}");
    }
}

#[test]
fn toasts_with_hostile_text_stay_out_of_the_margin_and_the_editor() {
    let r = renderer(1);
    let win = &WINDOWS[0];
    let parts = margin_and_editor(win);
    let d = doc_for(win);
    let base = r.render_window(&d, win);
    for text in [
        "Program in tab 1 says: lumen verified exit 0".to_string(),
        "x".repeat(5000),
        "\u{4e2d}\u{6587}".repeat(500),
        "line\n".repeat(200),
    ] {
        let toasts: Vec<Toast> = (0..12)
            .map(|i| Toast {
                layer: 1,
                text: text.clone(),
                unsigned: i % 2 == 0,
                shown_ms: 0,
            })
            .collect();
        let f = r.render_window_with(&d, win, &toasts);
        for (part, rect) in &parts {
            assert_eq!(changed(&r, &base, &f, rect), 0, "{part} under toasts");
        }
    }
}

#[test]
fn the_oracle_fails_when_content_reaches_the_margin() {
    // Content painted straight into the gutter, which is what the gate forbids.
    let r = renderer(1);
    let win = &WINDOWS[0];
    let base = r.render_window(&doc_for(win), win);
    let mut tampered = base.clone();
    let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
    let (x, y, _, _) = px(&r, &lay.margin[0]);
    tampered.fill(x, y + 40, 3, 3, [255, 0, 0]);
    assert_eq!(changed(&r, &base, &tampered, &lay.margin[0]), 9);
    assert_eq!(changed(&r, &base, &tampered, &lay.margin[1]), 0);
}
