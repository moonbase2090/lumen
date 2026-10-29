//! Cost mode: counting rules and the repository's own numbers.

use lumen_evidence::cost::{count_source, external_deps, measure, run_cost};
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
    let Ok((report, header)) = run_cost(&root(), "testsha", Vec::new()) else {
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
}
