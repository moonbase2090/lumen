//! Cost mode: counting rules and the repository's own numbers.

use lumen_evidence::cost::{
    count_source, count_vectors, external_deps, measure, parse_numstat, run_cost,
};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn lines_tests_and_unsafe_are_counted_by_the_stated_rules() {
    let text = "\
#![forbid(unsafe_code)]
// unsafe in a comment
fn a() {}

#[test]
fn t() {}
unsafe fn u() {}
extern \"C\" fn e() {}
let unsafely = 1;
#[cfg(test)]
mod tests {
    #[test]
    fn inner() {}
}
";
    let c = count_source("x.rs", text);
    assert_eq!(c.code, 7, "{c:?}");
    assert_eq!(c.inline_test, 5);
    assert_eq!(c.tests, 2);
    assert_eq!(c.unsafe_uses, 2);
}

#[test]
fn only_non_path_dependencies_are_external() {
    let m = "[package]\nname = \"x\"\n\n[dependencies]\nserde = \"1\"\nlumen-a = { path = \"../a\" }\n\n[dev-dependencies]\nrand = \"0.8\"\n\n[lints]\nworkspace = true\n";
    let d = external_deps(m);
    assert_eq!(
        d.iter().map(String::as_str).collect::<Vec<_>>(),
        ["rand", "serde"]
    );
}

#[test]
fn a_dependency_the_baseline_lacks_is_new() {
    let dir = std::env::temp_dir().join(format!("lumen-cost-{}", std::process::id()));
    let krate = |name: &str, deps: &str| {
        let d = dir.join("crates").join(name);
        assert!(std::fs::create_dir_all(d.join("src")).is_ok());
        assert!(std::fs::write(d.join("Cargo.toml"), format!("[dependencies]\n{deps}")).is_ok());
        assert!(
            std::fs::write(d.join("src/lib.rs"), "#![forbid(unsafe_code)]\nfn a() {}\n").is_ok()
        );
    };
    krate("lumen-detector", "memchr = \"2\"\n");
    krate("lumen-track0", "");
    krate("lumen-sealed", "");
    krate("lumen-arm-a", "memchr = \"2\"\nextra = \"1\"\n");
    krate("lumen-arm-b", "");
    let Ok(a) = measure(&dir, "arm-a", "lumen-arm-a") else {
        unreachable!("the tree is readable")
    };
    assert_eq!(a.new_deps.iter().collect::<Vec<_>>(), ["extra"]);
    assert!(a.forbids_unsafe);
    assert_eq!(a.fuzz_targets, 0);
}

#[test]
fn the_repository_has_no_unsafe_and_both_arms_are_measured() {
    let Ok((report, header)) = run_cost(&root(), "HEAD", "HEAD", Vec::new()) else {
        unreachable!("the repository is readable")
    };
    assert_eq!(report.arms.len(), 2);
    for a in &report.arms {
        assert!(a.src_lines() > 0 && a.test_fns() > 0, "{a:?}");
        assert_eq!(a.unsafe_uses(), 0, "{}", a.arm);
        assert!(a.forbids_unsafe, "{}", a.arm);
    }
    let s = report.summary(&header);
    assert!(s.contains("2.8 cost is reported only"), "{s}");
    assert!(s.contains("diff base=HEAD"), "{s}");
    assert!(report.vectors.total() > 0);
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "git {args:?}");
}

#[test]
fn lines_added_come_from_a_diff_between_two_commits() {
    let dir = std::env::temp_dir().join(format!("lumen-cost-git-{}", std::process::id()));
    for name in [
        "lumen-detector",
        "lumen-track0",
        "lumen-sealed",
        "lumen-arm-a",
        "lumen-arm-b",
    ] {
        let d = dir.join("crates").join(name);
        assert!(std::fs::create_dir_all(d.join("src")).is_ok());
        assert!(std::fs::write(d.join("Cargo.toml"), "[dependencies]\n").is_ok());
        assert!(std::fs::write(d.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").is_ok());
    }
    let v = dir.join("conformance/vectors");
    assert!(std::fs::create_dir_all(&v).is_ok());
    assert!(std::fs::write(v.join("x.json"), "{\"vectors\": []}\n").is_ok());
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    let arm = dir.join("crates/lumen-arm-a/src/lib.rs");
    assert!(std::fs::write(&arm, "#![forbid(unsafe_code)]\nfn a() {}\nfn b() {}\n").is_ok());
    let det = dir.join("crates/lumen-detector/src/lib.rs");
    assert!(std::fs::write(&det, "#![forbid(unsafe_code)]\nfn d() {}\n").is_ok());
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "arm"]);
    let Ok((report, _)) = run_cost(&dir, "HEAD~1", "HEAD", Vec::new()) else {
        unreachable!("the temporary repository is readable")
    };
    assert_eq!(report.added.arms, [2, 0]);
    assert_eq!(report.added.shared, 1);
    assert_eq!(report.added.vectors, 0);
}

#[test]
fn unsafe_in_a_string_or_a_trailing_comment_is_not_a_use() {
    let text = "\
let a = \"unsafe and extern\";
let b = 1; // unsafe
let c = \"quote \\\" unsafe\"; // extern
unsafe { x() }
";
    let c = count_source("x.rs", text);
    assert_eq!(c.unsafe_uses, 1, "{c:?}");
}

#[test]
fn numstat_lines_are_grouped_by_arm_shared_crates_and_vectors() {
    let n = "10\t2\tcrates/lumen-arm-a/src/lib.rs\n\
5\t0\tcrates/lumen-arm-b/tests/x.rs\n\
7\t1\tcrates/lumen-detector/src/lib.rs\n\
3\t0\tconformance/vectors/framing.json\n\
99\t0\tdocs/x.md\n\
-\t-\tcrates/lumen-arm-a/src/blob.rs\n";
    let Ok(a) = parse_numstat("base", n) else {
        unreachable!("well-formed")
    };
    assert_eq!(a.arms, [10, 5]);
    assert_eq!(a.shared, 7);
    assert_eq!(a.vectors, 3);
    assert!(parse_numstat("base", "oops").is_err());
}

#[test]
fn the_committed_vectors_are_counted_per_file() {
    let Ok(v) = count_vectors(&root()) else {
        unreachable!("the vectors are readable")
    };
    assert!(
        v.files.iter().any(|(f, n)| f == "framing.json" && *n > 0),
        "{v:?}"
    );
}
