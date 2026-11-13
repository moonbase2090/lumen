//! The design tokens: the floors every palette keeps, and that the terminal
//! draws with them.

use lumen_term::Theme;
use lumen_term::design::{
    self, Builtin, Family, Floors, Palette, RunColour, TYPE_SCALE, contrast, default_step, size_px,
};

/// Each built-in palette with the text, ANSI and border floors it keeps and
/// the ANSI slots exempt by convention, written out here, not taken from the
/// code under test.
fn builtins() -> [(&'static str, Palette, f64, f64, &'static [usize]); 3] {
    [
        ("light", Palette::light(), 4.5, 3.0, &[7, 15]),
        ("dark", Palette::dark(), 4.5, 3.0, &[0]),
        (
            "high contrast",
            Palette::high_contrast(),
            7.0,
            4.5,
            &[7, 15],
        ),
    ]
}

fn surfaces(p: &Palette) -> [(&'static str, [u8; 3]); 6] {
    [
        ("canvas", p.canvas),
        ("surface", p.surface),
        ("sunken", p.sunken),
        ("field", p.field),
        ("chrome", p.chrome),
        ("notice", p.notice),
    ]
}

fn text_roles(p: &Palette) -> [(&'static str, [u8; 3]); 7] {
    [
        ("text", p.text),
        ("text_strong", p.text_strong),
        ("text_muted", p.text_muted),
        ("chrome_text", p.chrome_text),
        ("accent", p.accent),
        ("error", p.error),
        ("caution", p.caution),
    ]
}

#[test]
fn contrast_matches_the_wcag_reference_values() {
    assert!((contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
    assert!((contrast([255, 255, 255], [255, 255, 255]) - 1.0).abs() < 1e-9);
    // #767676 on white is the well-known 4.54:1.
    let grey = contrast([118, 118, 118], [255, 255, 255]);
    assert!((grey - 4.54).abs() < 0.01, "{grey}");
    assert!((contrast([118, 118, 118], [255, 255, 255]) - grey).abs() < 1e-12);
}

#[test]
fn every_text_role_clears_its_palettes_floor_on_every_surface() {
    for (palette, p, floor, _, _) in builtins() {
        for (name, fg) in text_roles(&p) {
            for (surface, bg) in surfaces(&p) {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= floor,
                    "{palette}: {name} on {surface} is {ratio:.2}:1"
                );
            }
        }
    }
}

#[test]
fn the_run_colour_tokens_clear_the_specs_visibility_floor_with_room() {
    for (palette, p, _, _, _) in builtins() {
        for token in [
            RunColour::Fg,
            RunColour::Muted,
            RunColour::Accent,
            RunColour::Error,
        ] {
            for (surface, bg) in surfaces(&p) {
                let ratio = contrast(p.run(token), bg);
                assert!(
                    ratio >= 4.5,
                    "{palette}: {token:?} on {surface} is {ratio:.2}:1"
                );
            }
        }
    }
}

#[test]
fn ansi_text_colours_are_readable_on_the_canvas_and_the_surface() {
    for (palette, p, _, floor, exempt) in builtins() {
        for (i, colour) in p.ansi.iter().enumerate() {
            if exempt.contains(&i) {
                continue;
            }
            for (surface, bg) in [("canvas", p.canvas), ("surface", p.surface)] {
                let ratio = contrast(*colour, bg);
                assert!(
                    ratio >= floor,
                    "{palette}: ANSI {i} on {surface} is {ratio:.2}:1"
                );
            }
        }
        // The default foreground's ANSI slot is the readable one of black
        // and white.
        let readable = if p.is_dark() { p.ansi[15] } else { p.ansi[0] };
        assert!(contrast(readable, p.canvas) >= 4.5, "{palette}");
    }
}

#[test]
fn the_editor_and_container_edges_are_visible_as_control_boundaries() {
    for (palette, p, _, _, _) in builtins() {
        for (surface, bg) in [
            ("canvas", p.canvas),
            ("field", p.field),
            ("surface", p.surface),
        ] {
            let ratio = contrast(p.control_border, bg);
            assert!(
                ratio >= 3.0,
                "{palette}: control border on {surface} is {ratio:.2}:1"
            );
        }
    }
    assert_eq!(design::theme().border, Palette::light().control_border);
}

#[test]
fn dark_is_dark_and_the_others_are_light() {
    assert!(Palette::dark().is_dark());
    assert!(!Palette::light().is_dark());
    assert!(!Palette::high_contrast().is_dark());
}

#[test]
fn builtins_are_found_by_their_names() {
    assert_eq!(Builtin::named("light"), Some(Builtin::Light));
    assert_eq!(Builtin::named("dark"), Some(Builtin::Dark));
    assert_eq!(Builtin::named("high-contrast"), Some(Builtin::HighContrast));
    assert_eq!(Builtin::named("Dark"), None);
    assert_eq!(Builtin::named(""), None);
    assert_eq!(Builtin::Dark.palette(), Palette::dark());
    assert_eq!(Builtin::HighContrast.floors(), Floors::HIGH_CONTRAST);
}

#[test]
fn every_builtin_passes_its_own_check() {
    for b in Builtin::ALL {
        assert_eq!(
            b.palette().check(&b.floors()),
            Vec::<String>::new(),
            "{b:?}"
        );
    }
}

#[test]
fn the_check_names_each_miss() {
    let mut p = Palette::light();
    p.text_muted = [200, 200, 200];
    p.ansi[1] = [250, 250, 250];
    p.control_border = [240, 240, 240];
    let misses = p.check(&Floors::STANDARD);
    assert!(
        misses
            .iter()
            .any(|m| m.starts_with("text_muted on canvas is 1.67:1")),
        "{misses:?}"
    );
    assert!(
        misses.iter().any(|m| m.starts_with("ANSI 1 on canvas")),
        "{misses:?}"
    );
    assert!(
        misses
            .iter()
            .any(|m| m.starts_with("control_border on canvas")),
        "{misses:?}"
    );
    // A theme file is held only to the spec's floor for text.
    let user = p.check(&Floors::USER);
    assert!(
        user.iter().all(|m| m.starts_with("text_muted on ")),
        "{user:?}"
    );
    assert_ne!(user, Vec::<String>::new());
    // Exempt slots are not checked: white 15 on a light canvas is fine.
    let mut q = Palette::light();
    q.ansi[15] = [255, 255, 255];
    assert_eq!(q.check(&Floors::STANDARD), Vec::<String>::new());
}

#[test]
fn the_theme_is_the_palette_by_role() {
    let p = Palette::light();
    let t = design::theme();
    assert_eq!(t.foreground, p.text);
    assert_eq!(t.background, p.canvas);
    assert_eq!(t.ansi, p.ansi);
    assert_eq!(t.label, p.caution);
    assert_eq!(t.heading, p.text_strong);
    assert_eq!(t.margin, p.chrome);
    assert_eq!(t.margin_text, p.chrome_text);
    assert_eq!(t.code, p.sunken);
    assert_eq!(t.container, p.surface);
    assert_eq!(t.editor, p.field);
    assert_eq!(t.toast, p.notice);
    assert_eq!(t.toast_border, p.notice_border);
    assert_eq!(t.prompt, p.accent);
    // The evidence theme is left as it was.
    assert_ne!(t, Theme::light());
}

#[test]
fn the_type_scale_rises_and_steps_are_clamped() {
    assert!(TYPE_SCALE.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(size_px(0), 14);
    assert_eq!(size_px(-1), 12);
    assert_eq!(size_px(-9), TYPE_SCALE[0]);
    assert_eq!(size_px(9), TYPE_SCALE[TYPE_SCALE.len() - 1]);
    assert!(size_px(default_step(Family::Heading)) > size_px(default_step(Family::Body)));
    assert_eq!(default_step(Family::Mono), default_step(Family::Body));
    // Every size is over the spec's 6 px floor for a visible run.
    assert!(TYPE_SCALE.iter().all(|px| *px >= 6));
}

/// The terminal's window is drawn in the design theme, not the evidence one:
/// the gutter of a real app's view is the chrome colour.
#[cfg(unix)]
#[test]
fn the_app_draws_in_the_design_theme() -> Result<(), Box<dyn std::error::Error>> {
    use lumen_term::{App, GUTTER_COLUMNS, Passthrough};
    let app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", "sleep 2"],
        1,
        400,
        300,
    )?;
    let frame = app.view();
    let (cell_w, _) = lumen_term::Renderer::light(1)?.cell();
    let gutter = frame
        .pixel(cell_w * GUTTER_COLUMNS / 2, frame.height / 2)
        .ok_or("no pixel")?;
    assert_eq!(gutter, design::theme().margin);
    assert_ne!(gutter, Theme::light().margin);
    Ok(())
}

/// The editor's prompt mark is drawn in the accent, so the place to type is
/// the one coloured thing in the chrome.
#[cfg(unix)]
#[test]
fn the_editor_prompt_is_drawn_in_the_accent() -> Result<(), Box<dyn std::error::Error>> {
    use lumen_term::app::{cells_for, window_for};
    use lumen_term::{App, Passthrough, layout};
    let app = App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", "sleep 2"],
        1,
        640,
        380,
    )?;
    let frame = app.view();
    let (cw, ch) = lumen_term::Renderer::new(1, design::theme())?.cell();
    let (cols, rows) = cells_for(640, 380, (cw, ch));
    let win = window_for(cols, rows);
    let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
    // The mark sits in the editor's second cell of its first row.
    let (x0, y0) = ((lay.editor.x + 1) * cw, lay.editor.y * ch);
    // Find the most inked pixel of the mark. Glyph edges are blended, so it is
    // the accent laid over the field at some coverage: every channel must sit
    // on the line from the field to the accent at the same coverage, which
    // body text (a near-neutral grey) cannot do.
    let p = Palette::light();
    let mut darkest = p.field;
    for y in y0..y0 + ch {
        for x in x0..x0 + cw {
            let px = frame.pixel(x, y).ok_or("outside the frame")?;
            if px.iter().map(|c| u32::from(*c)).sum::<u32>()
                < darkest.iter().map(|c| u32::from(*c)).sum::<u32>()
            {
                darkest = px;
            }
        }
    }
    let span = |k: usize| f64::from(p.field[k]) - f64::from(p.accent[k]);
    let coverage = (f64::from(p.field[0]) - f64::from(darkest[0])) / span(0);
    assert!(coverage > 0.5, "the mark is barely drawn: {darkest:?}");
    for k in 1..3 {
        let want = f64::from(p.field[k]) - coverage * span(k);
        assert!(
            (f64::from(darkest[k]) - want).abs() <= 3.0,
            "{darkest:?} is not the accent {:?} at coverage {coverage:.2}",
            p.accent
        );
    }
    Ok(())
}
