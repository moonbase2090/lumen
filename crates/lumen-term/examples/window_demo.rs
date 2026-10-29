//! Renders a window with background output in the strip to a PNG, and prints
//! the changed pixels in the margin, the editor and the strip against a
//! baseline with no producer input:
//! `cargo run -p lumen-term --example window_demo -- out.png [scale]`.

// Short names for frames and rectangles keep the comparisons readable.
#![allow(clippy::many_single_char_names)]

use lumen_term::{Context, Document, Passthrough, Piece, Renderer, StripPlacement, Window, layout};
use lumen_track0::Block;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: window_demo OUT.png [SCALE]");
        std::process::exit(2);
    };
    let scale: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let Ok(renderer) = Renderer::light(scale) else {
        eprintln!("the bundled face did not load");
        std::process::exit(1);
    };
    let win = Window {
        cols: 80,
        rows: 24,
        editor_row: 19,
        editor_rows: 2,
    };
    let empty = Document::new(76, 18);
    let baseline = renderer.render_window(&empty, &win);

    let mut doc = Document::new(76, 4);
    doc.receive(
        &mut Passthrough,
        b"$ make\r\nbuilt in 2.1s\r\n",
        Context::default(),
    );
    doc.apply(vec![
        Piece::Rich(Block::Heading {
            level: 1,
            text: "Build report".into(),
        }),
        Piece::Background(Block::Paragraph("\u{2714} exit status 0 (verified)".into())),
        Piece::Background(Block::Paragraph("lumen".into())),
    ]);
    let closed = renderer.render_window(&doc, &win);
    doc.user_opens_strip(0);
    let open = renderer.render_window(&doc, &win);

    let (cw, ch) = renderer.cell();
    let count = |a: &lumen_term::Frame, b: &lumen_term::Frame, x: u32, y: u32, w: u32, h: u32| {
        (y * ch..(y + h) * ch)
            .map(|py| {
                (x * cw..(x + w) * cw)
                    .filter(|px| a.pixel(*px, py) != b.pixel(*px, py))
                    .count()
            })
            .sum::<usize>()
    };
    for (name, f, is_open) in [("collapsed", &closed, false), ("open", &open, true)] {
        let lay = layout(
            win.cols,
            win.rows,
            win.editor_row,
            win.editor_rows,
            Some((is_open, 2)),
        );
        let margin: usize = lay
            .margin
            .iter()
            .map(|m| count(&baseline, f, m.x, m.y, m.w, m.h))
            .sum();
        let editor = count(
            &baseline,
            f,
            lay.editor.x,
            lay.editor.y,
            lay.editor.w,
            lay.editor.h,
        );
        let strip = match lay.strip {
            StripPlacement::Placed(r) => count(&baseline, f, r.x, r.y, r.w, r.h),
            _ => 0,
        };
        println!(
            "{name}: changed pixels vs baseline: margin={margin} editor={editor} strip={strip}"
        );
    }
    match open.to_png() {
        Ok(png) => {
            if std::fs::write(&out, png).is_err() {
                eprintln!("could not write {out}");
                std::process::exit(1);
            }
            println!("wrote {out} ({}x{})", open.width, open.height);
        }
        Err(e) => {
            eprintln!("png: {e}");
            std::process::exit(1);
        }
    }
}
