//! Renders a window with a selection to a PNG, and prints what a copy of it
//! carries: `cargo run -p lumen-term --example selection_demo -- out.png [scale]`.
//! The selection runs from the grid row into the second line of a wrapped
//! paragraph.

use lumen_term::select::{Pos, Selection, copy_selection};
use lumen_term::{Document, Passthrough, Piece, Renderer, Window, layer::Context};
use lumen_track0::Block;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: selection_demo OUT.png [SCALE]");
        std::process::exit(2);
    };
    let scale: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);
    let (cols, rows) = (64u32, 20u32);
    let mut doc = Document::new(40, 3);
    doc.receive(
        &mut Passthrough,
        b"$ ls\r\nnotes.txt  report.md\r\n",
        Context::default(),
    );
    doc.apply(vec![Piece::Rich(Block::Paragraph(
        "A paragraph from a program, long enough that it wraps onto a second and a third line in the container.".into(),
    ))]);
    let Ok(r) = Renderer::light(scale) else {
        eprintln!("the bundled face did not load");
        std::process::exit(1);
    };
    let win = Window {
        cols,
        rows,
        editor_row: rows - 3,
        editor_rows: 3,
    };
    let columns = usize::try_from(cols - lumen_term::GUTTER_COLUMNS).unwrap_or(1);
    let lines = r.flow(&doc, columns);
    let second = lines.iter().position(|l| !l.grid).map_or(0, |i| i + 1);
    let sel = Selection {
        anchor: Pos { line: 1, col: 6 },
        head: Pos {
            line: second,
            col: 12,
        },
    };
    if let Some(copied) = copy_selection(&lines, &sel) {
        println!("copy: {:?}", copied.text);
    }
    let frame = r.render_window_selected(&doc, &win, &[], Some(&sel));
    match frame.to_png() {
        Ok(png) => {
            if let Err(e) = std::fs::write(&out, png) {
                eprintln!("could not write {out}: {e}");
                std::process::exit(1);
            }
            println!("{}x{} -> {out}", frame.width, frame.height);
        }
        Err(e) => {
            eprintln!("could not encode: {e}");
            std::process::exit(1);
        }
    }
}
