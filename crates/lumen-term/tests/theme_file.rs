//! Theme files: what they may say, and what refuses them.

use lumen_term::design::Palette;
use lumen_term::theme_file::{MAX_BYTES, Refused, load, parse};

fn line_of(r: Result<Palette, Refused>) -> (usize, String) {
    match r {
        Err(Refused::Line { line, why }) => (line, why),
        other => unreachable!("expected a line refusal, got {other:?}"),
    }
}

#[test]
fn an_empty_file_is_the_light_palette() {
    assert_eq!(parse(""), Ok(Palette::light()));
    assert_eq!(parse("# nothing\n\n   \n"), Ok(Palette::light()));
}

#[test]
fn base_picks_the_built_in_and_keys_override_its_roles() {
    let p = parse(
        "# A dark theme with a green accent.\n\
         base = \"dark\"\n\
         accent = \"#7EE2a8\"\n\
         ansi.2 = \"#7ee2a8\"\n\
         ansi.15 = \"#ffffff\"\n",
    );
    let mut want = Palette::dark();
    want.accent = [0x7e, 0xe2, 0xa8];
    want.ansi[2] = [0x7e, 0xe2, 0xa8];
    want.ansi[15] = [255, 255, 255];
    assert_eq!(p, Ok(want));
    assert_eq!(
        parse("base = \"high-contrast\""),
        Ok(Palette::high_contrast())
    );
}

#[test]
fn every_role_can_be_set() {
    let roles = [
        "canvas",
        "surface",
        "sunken",
        "field",
        "chrome",
        "text",
        "text_strong",
        "text_muted",
        "chrome_text",
        "accent",
        "error",
        "caution",
        "control_border",
        "notice",
        "notice_border",
    ];
    // Black text roles on white surfaces; borders and ANSI anything.
    for role in roles {
        let white = ["canvas", "surface", "sunken", "field", "chrome", "notice"].contains(&role);
        let value = if white { "#ffffff" } else { "#000000" };
        assert!(parse(&format!("{role} = \"{value}\"")).is_ok(), "{role}");
    }
    for i in 0..16 {
        let p = parse(&format!("ansi.{i} = \"#010203\"")).map_err(|e| e.to_string());
        assert_eq!(p.map(|p| p.ansi[i]), Ok([1, 2, 3]), "ansi.{i}");
    }
}

#[test]
fn a_bad_line_refuses_the_whole_file_and_names_the_line() {
    let cases = [
        ("accent = \"#123456\"\nnonsense\n", 2, "not `key = value`"),
        ("glow = \"#123456\"", 1, "`glow` is not a colour role"),
        ("ansi.16 = \"#123456\"", 1, "`ansi.16` is not a colour role"),
        ("ansi.07 = \"#123456\"", 1, "`ansi.07` is not a colour role"),
        ("ansi.+7 = \"#123456\"", 1, "`ansi.+7` is not a colour role"),
        ("ansi. = \"#123456\"", 1, "`ansi.` is not a colour role"),
        (
            "accent = \"#12345\"",
            1,
            "`accent` is not a quoted \"#rrggbb\" colour",
        ),
        (
            "accent = \"#12345g\"",
            1,
            "`accent` is not a quoted \"#rrggbb\" colour",
        ),
        (
            "accent = #123456",
            1,
            "`accent` is not a quoted \"#rrggbb\" colour",
        ),
        (
            "accent = \"123456\"",
            1,
            "`accent` is not a quoted \"#rrggbb\" colour",
        ),
        (
            "accent = \"#1234567\"",
            1,
            "`accent` is not a quoted \"#rrggbb\" colour",
        ),
        (
            "\n# c\naccent = \"#123456\"\naccent = \"#654321\"",
            4,
            "`accent` is given twice",
        ),
        (
            "base = \"solarized\"",
            1,
            "`base` is not \"light\", \"dark\" or \"high-contrast\"",
        ),
        (
            "base = dark",
            1,
            "`base` is not \"light\", \"dark\" or \"high-contrast\"",
        ),
    ];
    for (text, line, why) in cases {
        assert_eq!(line_of(parse(text)), (line, why.to_owned()), "{text:?}");
    }
}

#[test]
fn a_theme_whose_text_would_be_hidden_is_refused() {
    // Light grey body text on the light canvas: 1.6:1, under the spec's 3:1.
    match parse("text = \"#cccccc\"") {
        Err(Refused::TooFaint(misses)) => {
            assert!(
                misses.iter().all(|m| m.starts_with("text on ")),
                "{misses:?}"
            );
            assert!(
                misses
                    .iter()
                    .any(|m| m.starts_with("text on canvas is 1.6")),
                "{misses:?}"
            );
        }
        other => unreachable!("{other:?}"),
    }
    // A dark canvas under the light palette's dark text.
    assert!(matches!(
        parse("canvas = \"#000000\""),
        Err(Refused::TooFaint(_))
    ));
    // Faint ANSI colours are the user's choice: not refused.
    assert!(parse("ansi.1 = \"#fefefe\"").is_ok());
}

#[test]
fn a_file_is_read_whole_or_refused() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("theme-file");
    std::fs::create_dir_all(&dir)?;
    let good = dir.join("good.toml");
    std::fs::write(&good, "base = \"dark\"\naccent = \"#7ee2a8\"\n")?;
    assert_eq!(load(&good).map(|p| p.accent), Ok([0x7e, 0xe2, 0xa8]));

    let big = dir.join("big.toml");
    let mut text = "# padding\n".repeat(usize::try_from(MAX_BYTES)? / 10 + 1);
    text.push_str("accent = \"#7ee2a8\"\n");
    std::fs::write(&big, text)?;
    assert_eq!(load(&big), Err(Refused::TooLarge));

    let binary = dir.join("binary.toml");
    std::fs::write(&binary, [0xff, 0xfe, b'\n'])?;
    assert!(matches!(load(&binary), Err(Refused::Line { line: 1, .. })));

    assert_eq!(load(&dir.join("missing.toml")), Err(Refused::Unreadable));
    assert_eq!(load(&dir), Err(Refused::Unreadable));
    Ok(())
}
