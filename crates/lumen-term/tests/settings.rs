//! The settings file: what it may say, what ignores it, and that the terminal
//! draws with what it says.

use lumen_term::design::{Builtin, FontFile, Palette, ThemeChoice};
use lumen_term::settings::{
    FontSetting, LookSettings, MAX_BYTES, Settings, ThemeSetting, load_from, parse,
};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("settings")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| unreachable!("tmp dir: {e}"));
    d
}

fn asset(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/fonts")
            .join(name),
    )
    .unwrap_or_default()
}

const HOME: &str = "/home/someone";

fn parsed(text: &str) -> Result<LookSettings, (usize, String)> {
    parse(text, Path::new("/etc/conf"), Some(Path::new(HOME)))
}

#[test]
fn an_empty_file_is_the_defaults() {
    assert_eq!(parsed(""), Ok(LookSettings::default()));
    assert_eq!(parsed("# nothing\n\n"), Ok(LookSettings::default()));
    let d = LookSettings::default();
    assert_eq!(d.theme, ThemeSetting::System);
    assert_eq!(
        (d.mono_font, d.prose_font),
        (FontSetting::Bundled, FontSetting::Bundled)
    );
    assert_eq!((d.mono_size, d.prose_size), (14, 14));
}

#[test]
fn every_setting_is_read() {
    let look = parsed(
        "# The look.\n\
         theme = \"dark\"  # comment after a value\n\
         mono_font = \"~/fonts/Mono.ttf\"\n\
         mono_size = 16\n\
         prose_font = \"/abs/Sans.otf\"\n\
         prose_size = 18\n",
    );
    assert_eq!(
        look,
        Ok(LookSettings {
            theme: ThemeSetting::Builtin(Builtin::Dark),
            mono_font: FontSetting::File(PathBuf::from("/home/someone/fonts/Mono.ttf")),
            mono_size: 16,
            prose_font: FontSetting::File(PathBuf::from("/abs/Sans.otf")),
            prose_size: 18,
        })
    );
}

#[test]
fn theme_names_and_paths() {
    let theme = |v: &str| parsed(&format!("theme = \"{v}\"")).map(|l| l.theme);
    assert_eq!(theme("system"), Ok(ThemeSetting::System));
    assert_eq!(theme("light"), Ok(ThemeSetting::Builtin(Builtin::Light)));
    assert_eq!(
        theme("high-contrast"),
        Ok(ThemeSetting::Builtin(Builtin::HighContrast))
    );
    // A path is relative to the settings file's directory unless it is
    // absolute or under the home directory.
    assert_eq!(
        theme("./mine.toml"),
        Ok(ThemeSetting::File("/etc/conf/./mine.toml".into()))
    );
    assert_eq!(
        theme("themes/mine.toml"),
        Ok(ThemeSetting::File("/etc/conf/themes/mine.toml".into()))
    );
    assert_eq!(
        theme("~/t.toml"),
        Ok(ThemeSetting::File("/home/someone/t.toml".into()))
    );
    assert_eq!(theme("/t.toml"), Ok(ThemeSetting::File("/t.toml".into())));
    // A bare name that is not a built-in is not a file.
    assert!(theme("mine.toml").is_err());
    assert!(theme("Dark").is_err());
    // Without a home directory, a ~/ path is refused.
    assert!(parse("theme = \"~/t.toml\"", Path::new("/c"), None).is_err());
}

#[test]
fn a_bad_line_ignores_the_whole_file_and_names_the_line() {
    let cases = [
        ("theme = \"dark\"\nnonsense\n", 2, "expected key = value"),
        ("glow = \"on\"", 1, "unknown setting `glow`"),
        ("theme = dark", 1, "`theme` needs a quoted value"),
        ("theme = \"da\"rk\"", 1, "`theme` needs a quoted value"),
        (
            "theme = \"solar\"",
            1,
            "`theme` must be system, light, dark, high-contrast or a path with a /",
        ),
        (
            "mono_font = \"Menlo\"",
            1,
            "`mono_font` must be \"bundled\" or a path with a /",
        ),
        (
            "mono_size = 7",
            1,
            "`mono_size` must be a whole number from 8 to 72",
        ),
        (
            "prose_size = 73",
            1,
            "`prose_size` must be a whole number from 8 to 72",
        ),
        (
            "prose_size = 14.5",
            1,
            "`prose_size` must be a whole number from 8 to 72",
        ),
        (
            "mono_size = \"14\"",
            1,
            "`mono_size` must be a whole number from 8 to 72",
        ),
        (
            "mono_size = 14\nmono_size = 15",
            2,
            "`mono_size` is given twice",
        ),
    ];
    for (text, line, why) in cases {
        assert_eq!(parsed(text), Err((line, why.to_owned())), "{text:?}");
    }
}

#[test]
fn a_file_that_cannot_be_used_is_ignored_with_a_notice() -> Result<(), Box<dyn std::error::Error>> {
    let d = dir("ignored");
    assert_eq!(load_from(&d.join("missing.toml"), None), Settings::Defaults);

    let bad = d.join("bad.toml");
    std::fs::write(&bad, "theme = \"dark\"\nglow = \"on\"\n")?;
    let s = load_from(&bad, None);
    assert_eq!(s.look(), LookSettings::default());
    let notice = s.notice().unwrap_or_default();
    assert!(
        notice.contains("line 2: unknown setting `glow`"),
        "{notice}"
    );
    assert!(notice.ends_with("the defaults apply"), "{notice}");

    let big = d.join("big.toml");
    std::fs::write(&big, "#".repeat(usize::try_from(MAX_BYTES)? + 1))?;
    assert!(
        load_from(&big, None)
            .notice()
            .unwrap_or_default()
            .contains("larger than")
    );

    let binary = d.join("binary.toml");
    std::fs::write(&binary, [0xff, 0xfe])?;
    assert!(
        load_from(&binary, None)
            .notice()
            .unwrap_or_default()
            .contains("not UTF-8")
    );

    assert!(
        load_from(&d, None)
            .notice()
            .unwrap_or_default()
            .contains("not a regular file")
    );

    let good = d.join("good.toml");
    std::fs::write(&good, "theme = \"high-contrast\"\n")?;
    let s = load_from(&good, None);
    assert_eq!(s.notice(), None);
    assert_eq!(s.look().theme, ThemeSetting::Builtin(Builtin::HighContrast));
    Ok(())
}

#[test]
fn files_the_settings_name_are_read_or_left_out_with_a_notice()
-> Result<(), Box<dyn std::error::Error>> {
    let d = dir("resolve");
    std::fs::write(
        d.join("green.toml"),
        "base = \"dark\"\naccent = \"#7ee2a8\"\n",
    )?;
    std::fs::write(d.join("faint.toml"), "text = \"#fefefe\"\n")?;
    std::fs::write(d.join("Mono.ttf"), asset("IBMPlexMono-Regular.ttf"))?;
    std::fs::write(d.join("junk.ttf"), b"junk")?;

    let good = parse(
        "theme = \"./green.toml\"\nmono_font = \"./Mono.ttf\"\nmono_size = 20\nprose_size = 9\n",
        &d,
        None,
    )
    .map_err(|e| format!("{e:?}"))?
    .resolve();
    let mut green = Palette::dark();
    green.accent = [0x7e, 0xe2, 0xa8];
    assert_eq!(good.theme, ThemeChoice::Fixed(green));
    assert!(matches!(good.fonts.mono, FontFile::Data(_)));
    assert_eq!(good.fonts.prose, FontFile::Bundled);
    assert_eq!((good.fonts.mono_px(), good.fonts.prose_px()), (20, 9));
    assert_eq!(good.notices, Vec::<String>::new());

    let bad = parse(
        "theme = \"./faint.toml\"\nmono_font = \"./junk.ttf\"\nprose_font = \"./none.ttf\"\nmono_size = 20\n",
        &d,
        None,
    )
    .map_err(|e| format!("{e:?}"))?
    .resolve();
    assert_eq!(bad.theme, ThemeChoice::System);
    assert_eq!(
        (bad.fonts.mono.clone(), bad.fonts.prose.clone()),
        (FontFile::Bundled, FontFile::Bundled)
    );
    // The rest of the settings still apply.
    assert_eq!(bad.fonts.mono_px(), 20);
    assert_eq!(bad.notices.len(), 3, "{:?}", bad.notices);
    assert!(
        bad.notices[0].contains("faint.toml not used (text would be too faint"),
        "{:?}",
        bad.notices
    );
    assert!(
        bad.notices[1].starts_with("mono font ") && bad.notices[1].contains("not a font"),
        "{:?}",
        bad.notices
    );
    assert!(
        bad.notices[2].starts_with("prose font ") && bad.notices[2].contains("could not be read"),
        "{:?}",
        bad.notices
    );
    Ok(())
}

/// The terminal reads the settings file from the home directory and draws
/// with it: a snapshot taken with `theme = "dark"` has the dark canvas and the
/// dark trust margin.
#[cfg(unix)]
#[test]
fn the_binary_draws_with_the_settings_file() -> Result<(), Box<dyn std::error::Error>> {
    let home = dir("binary-home");
    std::fs::create_dir_all(home.join(".config/lumen"))?;
    std::fs::write(
        home.join(".config/lumen/settings.toml"),
        "theme = \"dark\"\n",
    )?;
    let path = home.join("snap.png");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
        .env("HOME", &home)
        .args(["--size", "320x190", "--wait", "300", "--snapshot"])
        .arg(&path)
        .args(["--", "/bin/sh", "-c", "true"])
        .output()?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut reader =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path)?)).read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let at = |x: u32, y: u32| {
        let i = ((y * info.width + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    let dark = Palette::dark();
    // The gutter's left edge, halfway down; and the right edge of the content
    // area, halfway down.
    assert_eq!(at(2, info.height / 2), dark.chrome);
    assert_eq!(at(info.width - 3, info.height / 2), dark.canvas);

    // --show-settings says what was read.
    let shown = std::process::Command::new(env!("CARGO_BIN_EXE_lumen-term"))
        .env("HOME", &home)
        .arg("--show-settings")
        .output()?;
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(
        text.contains("settings.toml") && text.contains("theme=dark"),
        "{text}"
    );
    Ok(())
}
