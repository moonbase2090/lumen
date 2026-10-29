//! Pixel mode against the real terminal binary.

#![cfg(unix)]

use lumen_evidence::pixels_terminal::run_terminal;
use lumen_evidence::{Corpus, load_corpus};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> Corpus {
    load_corpus(&root().join("docs/phase-0.5/corpus-pixel"))
        .unwrap_or_else(|e| unreachable!("the pixel corpus loads: {e}"))
}

/// A few cases that draw (a paragraph flood, a plain scroll) and one that
/// carries a context the real terminal cannot be given.
fn subset(c: &Corpus) -> Corpus {
    let want = [
        "badge-glyphs",
        "many-blocks-200",
        "scroll-flood-2000-lines",
        "track0-under-tmux",
    ];
    let mut s = c.clone();
    s.cases.retain(|x| want.contains(&x.id.as_str()));
    s
}

/// Build the terminal binary and return its path.
fn terminal() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--locked", "-q", "-p", "lumen-term"])
        .current_dir(root())
        .status();
    assert!(status.is_ok_and(|s| s.success()), "the terminal builds");
    let dir =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from);
    dir.join("debug").join("lumen-term")
}

#[test]
fn the_real_terminal_measures_zero_and_draws_something() {
    let t = terminal();
    let run = run_terminal(&subset(&corpus()), 1, &t, None)
        .unwrap_or_else(|e| unreachable!("the run succeeds: {e}"));
    assert!(run.report.baseline_stable);
    assert_eq!(run.report.cases.len(), 3 * 2, "three cases in two windows");
    assert_eq!((run.report.margin(), run.report.chrome()), (0, 0));
    assert!(
        run.report.drawn() >= 4,
        "the cases reached the drawing: {:?}",
        run.report.cases
    );
    assert_eq!(run.report.exit_code(), 0);
    assert_eq!(run.skipped.len(), 1);
    assert!(
        run.skipped[0].starts_with("track0-under-tmux"),
        "{:?}",
        run.skipped
    );
}

/// A terminal that runs the real one but replaces the program with one that
/// prints nothing: every frame is the baseline.
fn drawing_nothing(real: &Path, dir: &Path) -> PathBuf {
    let script = dir.join("blank-terminal.sh");
    let body = format!(
        "#!/bin/sh\nargs=\"\"\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then break; fi\n  args=\"$args $1\"\n  shift\ndone\nexec '{}' $args -- /bin/sh -c ':'\n",
        real.display()
    );
    assert!(std::fs::write(&script, body).is_ok());
    assert!(
        Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status()
            .is_ok_and(|s| s.success())
    );
    script
}

#[test]
fn a_terminal_that_draws_nothing_is_not_measured_rather_than_a_zero() {
    let real = terminal();
    let dir = std::env::temp_dir().join(format!("lumen-blank-term-{}", std::process::id()));
    assert!(std::fs::create_dir_all(&dir).is_ok());
    let blank = drawing_nothing(&real, &dir);
    let Ok(run) = run_terminal(&subset(&corpus()), 1, &blank, None) else {
        unreachable!("the run succeeds")
    };
    assert_eq!((run.report.margin(), run.report.chrome()), (0, 0));
    assert_eq!(run.report.drawn(), 0);
    assert_eq!(
        run.report.exit_code(),
        3,
        "a zero over nothing drawn is not a pass"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_terminal_is_an_error_not_a_zero() {
    let r = run_terminal(
        &subset(&corpus()),
        1,
        Path::new("/nonexistent/lumen-term"),
        None,
    );
    assert!(r.is_err());
}
