//! The user's fonts: which faces, what sizes, and that a change takes effect
//! with no restart.

use lumen_term::Renderer;
use lumen_term::design::{
    self, FontFile, FontRefused, Fonts, MAX_FONT_BYTES, MAX_FONT_PX, MIN_FONT_PX,
};

fn asset(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/fonts")
        .join(name);
    std::fs::read(path).unwrap_or_default()
}

fn renderer(fonts: &Fonts) -> Renderer {
    Renderer::with_fonts(1, design::theme(), fonts)
        .unwrap_or_else(|e| unreachable!("the faces load: {e}"))
}

#[test]
fn the_defaults_are_the_bundled_faces_at_the_body_size() {
    let f = Fonts::default();
    assert_eq!(
        (f.mono.clone(), f.prose.clone()),
        (FontFile::Bundled, FontFile::Bundled)
    );
    assert_eq!((f.mono_px(), f.prose_px()), (14, 14));
    // The default renderer is the one the evidence and the window have always used.
    assert_eq!(
        renderer(&f).cell(),
        Renderer::light(1).map(|r| r.cell()).unwrap_or_default()
    );
}

#[test]
fn sizes_are_clamped_to_a_readable_range() {
    let f = Fonts::default().with_mono_px(1).with_prose_px(500);
    assert_eq!((f.mono_px(), f.prose_px()), (MIN_FONT_PX, MAX_FONT_PX));
}

#[test]
fn a_larger_mono_size_makes_larger_cells() {
    let (w14, h14) = renderer(&Fonts::default()).cell();
    let (w28, h28) = renderer(&Fonts::default().with_mono_px(28)).cell();
    // Twice the size, twice the cell, give or take rounding.
    assert!(w28.abs_diff(2 * w14) <= 1, "{w14} -> {w28}");
    assert!(h28.abs_diff(2 * h14) <= 1, "{h14} -> {h28}");
}

#[test]
fn the_prose_size_sets_proportional_widths_and_rows_fit_it() {
    let base = renderer(&Fonts::default());
    let big = renderer(&Fonts::default().with_prose_px(28));
    let (a, b) = (
        base.text_width("Hello, world"),
        big.text_width("Hello, world"),
    );
    // Twice the size, about twice the width (in 64ths of a pixel).
    assert!(b.abs_diff(2 * a) <= 64, "{a} -> {b}");
    // Cells stay as wide as the monospace face, and rows grow to fit prose.
    assert_eq!(big.cell().0, base.cell().0);
    assert!(
        big.cell().1 >= 2 * base.cell().1 - 1,
        "{:?} -> {:?}",
        base.cell(),
        big.cell()
    );
}

#[test]
fn a_users_face_is_the_one_drawn() {
    // The proportional face as the monospace one: an `M` in it is wider than
    // the bundled monospace `M`, so the cell is wider.
    let sans = FontFile::from_bytes(asset("IBMPlexSans-Regular.ttf"));
    let Ok(sans) = sans else {
        unreachable!("the bundled face is a font: {sans:?}")
    };
    let fonts = Fonts::default().with_mono(sans);
    assert_ne!(
        renderer(&fonts).cell().0,
        renderer(&Fonts::default()).cell().0
    );
}

#[test]
fn what_is_not_a_font_is_refused() {
    assert_eq!(FontFile::from_bytes(Vec::new()), Err(FontRefused::NotAFont));
    assert_eq!(
        FontFile::from_bytes(b"not a font at all".to_vec()),
        Err(FontRefused::NotAFont)
    );
    // A real font's header with the tables cut off.
    let mut cut = asset("IBMPlexMono-Regular.ttf");
    cut.truncate(64);
    assert_eq!(FontFile::from_bytes(cut), Err(FontRefused::NotAFont));
}

#[test]
fn a_font_file_is_read_whole_or_refused() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("font-file");
    std::fs::create_dir_all(&dir)?;
    let good = dir.join("mono.ttf");
    std::fs::write(&good, asset("IBMPlexMono-Regular.ttf"))?;
    assert!(matches!(FontFile::load(&good), Ok(FontFile::Data(_))));

    let junk = dir.join("junk.ttf");
    std::fs::write(&junk, b"junk")?;
    assert_eq!(FontFile::load(&junk), Err(FontRefused::NotAFont));

    let big = dir.join("big.ttf");
    std::fs::File::create(&big)?.set_len(MAX_FONT_BYTES + 1)?;
    assert_eq!(FontFile::load(&big), Err(FontRefused::TooLarge));

    assert_eq!(
        FontFile::load(&dir.join("missing.ttf")),
        Err(FontRefused::Unreadable)
    );
    assert_eq!(FontFile::load(&dir), Err(FontRefused::Unreadable));
    Ok(())
}

/// A font change takes effect with no restart: the program's grid is resized
/// to the cells the new size gives.
#[cfg(unix)]
#[test]
fn a_font_change_resizes_the_grid_live() -> Result<(), Box<dyn std::error::Error>> {
    use lumen_term::app::{block_cells, cells_for};
    use lumen_term::{App, Passthrough};
    let mut app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", "sleep 3"],
        1,
        640,
        400,
    )?;
    let before = app.session().document().columns();
    let bigger = Fonts::default().with_mono_px(28);
    assert!(app.set_fonts(&bigger)?);
    let (cols, rows) = cells_for(640, 400, renderer(&bigger).cell());
    assert_eq!(
        (
            app.session().document().columns(),
            app.session().document().rows()
        ),
        block_cells(cols, rows)
    );
    assert!(app.session().document().columns() < before);
    // The same fonts again change nothing.
    assert!(!app.set_fonts(&bigger)?);
    Ok(())
}
