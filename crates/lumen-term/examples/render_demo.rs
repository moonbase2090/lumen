//! Renders a small document to a PNG: `cargo run -p lumen-term --example render_demo -- out.png [scale]`.

use lumen_term::{Document, Passthrough, Piece, Renderer, layer::Context};
use lumen_track0::Block;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: render_demo OUT.png [SCALE]");
        std::process::exit(2);
    };
    let scale: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut doc = Document::new(40, 4);
    doc.receive(
        &mut Passthrough,
        b"$ ls\r\n\x1b[31mred\x1b[0m \x1b[1;32mgreen\x1b[0m \x1b[7minverse\x1b[0m\r\n\x1b[4munderlined\x1b[0m",
        Context::default(),
    );
    doc.apply(vec![
        Piece::Rich(Block::Heading {
            level: 1,
            text: "Build report".into(),
        }),
        Piece::Rich(Block::Paragraph(
            "Everything in this box came from a program the terminal cannot vouch for, so it is labelled.".into(),
        )),
        Piece::Rich(Block::Code {
            lang: "sh".into(),
            text: "cargo test\nok".into(),
            stripped: 0,
        }),
    ]);
    doc.receive(&mut Passthrough, b"$ ", Context::default());
    let Ok(r) = Renderer::light(scale) else {
        eprintln!("the bundled face did not load");
        std::process::exit(1);
    };
    let frame = r.render(&doc);
    match frame.to_png() {
        Ok(png) => {
            if std::fs::write(&out, png).is_err() {
                eprintln!("could not write {out}");
                std::process::exit(1);
            }
            println!("{}x{} -> {out}", frame.width, frame.height);
        }
        Err(e) => {
            eprintln!("png: {e}");
            std::process::exit(1);
        }
    }
}
