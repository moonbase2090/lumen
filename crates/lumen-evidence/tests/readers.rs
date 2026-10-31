//! Reader screenshot mode: the committed prompt set renders, and the score is exact.

use lumen_evidence::readers::{load_prompts, render_prompts, score};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/phase-0.5/corpus-readers")
}

fn out(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lumen-readers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Every reader gives `said(prompt, rendering, truth)` for every shot.
fn labels(
    manifest: &Value,
    readers: usize,
    said: &dyn Fn(&str, &str, &str) -> &'static str,
) -> Value {
    let mut all = serde_json::Map::new();
    for n in 0..readers {
        let mut one = serde_json::Map::new();
        for s in manifest["shots"].as_array().into_iter().flatten() {
            let (p, r, t) = (
                s["prompt"].as_str().unwrap_or(""),
                s["rendering"].as_str().unwrap_or(""),
                s["label"].as_str().unwrap_or(""),
            );
            one.insert(format!("{p}/{r}"), json!(said(p, r, t)));
        }
        all.insert(format!("reader-{n}"), Value::Object(one));
    }
    Value::Object(all)
}

#[test]
fn the_committed_prompt_set_renders_in_both_renderings_with_a_manifest() {
    let Ok(prompts) = load_prompts(&dir()) else {
        unreachable!("the prompts load")
    };
    assert_eq!(prompts.revision, "rd-1");
    let o = out("render");
    let Ok(shots) = render_prompts(&prompts, 2, &o) else {
        unreachable!("it renders")
    };
    let n = shots.manifest["shots"].as_array().map_or(0, Vec::len);
    assert_eq!(n, prompts.prompts.len() * 2);
    for s in shots.manifest["shots"].as_array().into_iter().flatten() {
        let f = o.join("readers").join(s["file"].as_str().unwrap_or(""));
        let bytes = std::fs::read(&f).unwrap_or_default();
        assert!(bytes.starts_with(b"\x89PNG"), "{} is a PNG", f.display());
    }
    // A forged Track 0 block is drawn differently by Arm B than by the plain-VT
    // control; a plain-text spoof is drawn the same, which the run reports.
    assert!(!shots.same_frame.contains(&"spoof-badge-frame".to_owned()));
    assert!(shots.same_frame.contains(&"spoof-text-status".to_owned()));
    let _ = std::fs::remove_dir_all(&o);
}

#[test]
fn rendering_twice_gives_identical_screenshots() {
    let Ok(prompts) = load_prompts(&dir()) else {
        unreachable!("the prompts load")
    };
    let (a, b) = (out("a"), out("b"));
    let (Ok(x), Ok(y)) = (
        render_prompts(&prompts, 2, &a),
        render_prompts(&prompts, 2, &b),
    ) else {
        unreachable!("it renders")
    };
    assert_eq!(x.manifest["shots"], y.manifest["shots"], "same digests");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[test]
fn the_score_counts_accepted_spoofs_and_vetoes_an_arm_above_the_control() {
    let Ok(prompts) = load_prompts(&dir()) else {
        unreachable!("the prompts load")
    };
    let o = out("score");
    let Ok(shots) = render_prompts(&prompts, 2, &o) else {
        unreachable!("it renders")
    };
    let spoofed = prompts
        .prompts
        .iter()
        .filter(|p| p.label == lumen_evidence::readers::Label::Spoofed)
        .count();
    // Readers who are always right: nothing accepted, no veto.
    let right = labels(&shots.manifest, 10, &|_, _, t| {
        if t == "spoofed" { "spoofed" } else { "genuine" }
    });
    let Ok(s) = score(&shots.manifest, &right) else {
        unreachable!("scores")
    };
    assert_eq!(s.readers, 10);
    assert_eq!(s.tallies["arm-b"].spoofed_shown, spoofed * 10);
    assert_eq!(s.tallies["arm-b"].accepted, 0);
    assert_eq!(s.vetoed(), Some(false));
    // Readers fooled by every Arm B shot and by none of the control's: veto.
    let fooled = labels(&shots.manifest, 10, &|_, r, t| {
        if t == "spoofed" && r == "arm-b" {
            "genuine"
        } else if t == "spoofed" {
            "spoofed"
        } else {
            "genuine"
        }
    });
    let Ok(s) = score(&shots.manifest, &fooled) else {
        unreachable!("scores")
    };
    assert_eq!(s.tallies["arm-b"].accepted, spoofed * 10);
    assert_eq!(s.tallies["plain-vt"].accepted, 0);
    assert_eq!(s.vetoed(), Some(true));
    assert!(s.summary().contains("veto: arm-b is above"));
    let _ = std::fs::remove_dir_all(&o);
}

#[test]
fn labels_that_are_missing_unknown_or_not_a_label_are_refused() {
    let Ok(prompts) = load_prompts(&dir()) else {
        unreachable!("the prompts load")
    };
    let o = out("refuse");
    let Ok(shots) = render_prompts(&prompts, 2, &o) else {
        unreachable!("it renders")
    };
    let mut missing = labels(&shots.manifest, 1, &|_, _, _| "genuine");
    if let Some(r) = missing["reader-0"].as_object_mut() {
        r.remove("genuine-fresh/arm-b");
    }
    assert!(score(&shots.manifest, &missing).is_err_and(|e| e.contains("no label for")));
    let mut unknown = labels(&shots.manifest, 1, &|_, _, _| "genuine");
    unknown["reader-0"]["nope/arm-b"] = json!("genuine");
    assert!(score(&shots.manifest, &unknown).is_err_and(|e| e.contains("not a shot")));
    let bad = labels(&shots.manifest, 1, &|_, _, _| "maybe");
    assert!(score(&shots.manifest, &bad).is_err_and(|e| e.contains("genuine or spoofed")));
    assert!(score(&shots.manifest, &json!({})).is_err());
    let _ = std::fs::remove_dir_all(&o);
}
