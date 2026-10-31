//! The reader mode through the binary: exit 3 without labels, the veto as exit 1.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_lumen-evidence")
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-readers")
}

#[test]
fn without_labels_the_screenshots_are_written_and_the_metric_is_not_measured() {
    let o = std::env::temp_dir().join(format!("lumen-readers-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&o);
    let r = Command::new(bin())
        .args(["--readers"])
        .arg(corpus())
        .args(["--commit", "testsha", "--out"])
        .arg(&o)
        .output();
    let Ok(r) = r else {
        unreachable!("the binary runs")
    };
    assert_eq!(r.status.code(), Some(3));
    let s = std::fs::read_to_string(o.join("summary.txt")).unwrap_or_default();
    assert!(s.contains("2.2b=not measured"), "{s}");
    assert!(o.join("readers/manifest.json").exists());
    let _ = std::fs::remove_dir_all(&o);
}
