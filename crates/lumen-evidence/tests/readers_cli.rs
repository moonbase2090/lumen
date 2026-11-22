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

#[test]
fn labels_that_veto_arm_a_exit_1() {
    let o = std::env::temp_dir().join(format!("lumen-readers-cli-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&o);
    let run = |labels: Option<&Path>| {
        let mut c = Command::new(bin());
        c.args(["--readers"])
            .arg(corpus())
            .args(["--commit", "testsha", "--out"])
            .arg(&o);
        if let Some(l) = labels {
            c.arg("--labels").arg(l);
        }
        c.output()
    };
    let Ok(first) = run(None) else {
        unreachable!("the binary runs")
    };
    assert_eq!(first.status.code(), Some(3));
    let raw = std::fs::read_to_string(o.join("readers/manifest.json")).unwrap_or_default();
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        unreachable!("the manifest is JSON")
    };
    // One reader, fooled by every spoofed Arm A shot and by no other.
    let mut one = serde_json::Map::new();
    for s in manifest["shots"].as_array().into_iter().flatten() {
        let key = format!(
            "{}/{}",
            s["prompt"].as_str().unwrap_or(""),
            s["rendering"].as_str().unwrap_or("")
        );
        let said = if s["label"] == "spoofed" && s["rendering"] != "arm-a" {
            "spoofed"
        } else {
            "genuine"
        };
        one.insert(key, serde_json::json!(said));
    }
    let labels = o.with_extension("labels.json");
    let body = serde_json::json!({ "reader-1": one }).to_string();
    assert!(std::fs::write(&labels, body).is_ok());
    let Ok(second) = run(Some(&labels)) else {
        unreachable!("the binary runs")
    };
    assert_eq!(second.status.code(), Some(1));
    let s = std::fs::read_to_string(o.join("summary.txt")).unwrap_or_default();
    assert!(
        s.contains("2.2b veto: arm-a is above the plain-VT control"),
        "{s}"
    );
    assert!(s.contains("2.2b veto: arm-b not triggered"), "{s}");
    let _ = std::fs::remove_dir_all(&o);
    let _ = std::fs::remove_file(&labels);
}
