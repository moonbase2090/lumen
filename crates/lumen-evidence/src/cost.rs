//! Cost mode (criteria 2.8): what each arm adds to the shared baseline.
//!
//! The arms are separate crates over the same shared crates. The run takes a
//! base commit and the measured commit and reports, from a diff of the two
//! (`git diff --numstat`), the lines added in each arm crate and, apart, in
//! the shared crates, whose edits belong to neither arm. From the tree at the
//! measured commit it reports, per arm: code lines in `src` and in `tests`,
//! test functions, new dependencies, `unsafe` and FFI uses and fuzz targets.
//! It also counts the conformance vectors, which gate both arms. Nothing is
//! estimated: every number is a count over files or over the diff, and the
//! per-file counts are in the summary.
//!
//! Counting rules: a code line is a line that is not blank and does not start
//! with `//`. Block comments are not recognised, so a line inside one counts.
//! Lines after a top-level `#[cfg(test)]` in a `src` file are test lines.
//! Test functions are `#[test]` attributes. A use of `unsafe` or `extern` is
//! a line with either word outside a trailing `//` comment and outside a
//! string literal; raw strings are not recognised.

use lumen_harness::{EffectLog, HeaderInfo, Recorder, SinkTable};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::HARNESS_VERSION;

/// The two arm crates, as directory names under `crates/`.
pub const ARMS: [(&str, &str); 2] = [("arm-a", "lumen-arm-a"), ("arm-b", "lumen-arm-b")];

/// The crates both arms share. Their external dependencies are the baseline.
pub const SHARED: [&str; 3] = ["lumen-detector", "lumen-track0", "lumen-sealed"];

/// Counts for one `.rs` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCount {
    /// Path relative to the crate directory.
    pub path: String,
    /// Code lines outside a top-level `#[cfg(test)]` module.
    pub code: usize,
    /// Code lines after a top-level `#[cfg(test)]`.
    pub inline_test: usize,
    /// `#[test]` attributes.
    pub tests: usize,
    /// Uses of `unsafe` or `extern`, outside comments.
    pub unsafe_uses: usize,
}

/// What one arm adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmCost {
    /// `arm-a` or `arm-b`.
    pub arm: &'static str,
    /// Per-file counts for `src`.
    pub src: Vec<FileCount>,
    /// Per-file counts for `tests`.
    pub tests: Vec<FileCount>,
    /// External (not path) dependencies of the arm, with dev-dependencies.
    pub external_deps: BTreeSet<String>,
    /// External dependencies that the shared crates do not already have.
    pub new_deps: BTreeSet<String>,
    /// Every crate root has `#![forbid(unsafe_code)]` or the workspace forbids it.
    pub forbids_unsafe: bool,
    /// Fuzz targets under the crate's `fuzz` directory.
    pub fuzz_targets: usize,
}

impl ArmCost {
    /// Code lines in `src`.
    #[must_use]
    pub fn src_lines(&self) -> usize {
        self.src.iter().map(|f| f.code).sum()
    }

    /// Test code lines: the `tests` directory plus inline test modules.
    #[must_use]
    pub fn test_lines(&self) -> usize {
        self.tests.iter().map(|f| f.code).sum::<usize>()
            + self.src.iter().map(|f| f.inline_test).sum::<usize>()
    }

    /// Test functions in `src` and `tests`.
    #[must_use]
    pub fn test_fns(&self) -> usize {
        self.src.iter().chain(&self.tests).map(|f| f.tests).sum()
    }

    /// Uses of `unsafe` or `extern` in `src` and `tests`.
    #[must_use]
    pub fn unsafe_uses(&self) -> usize {
        self.src
            .iter()
            .chain(&self.tests)
            .map(|f| f.unsafe_uses)
            .sum()
    }
}

fn is_code(line: &str) -> bool {
    let t = line.trim_start();
    !t.is_empty() && !t.starts_with("//")
}

fn has_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// The line without string literal contents and without a trailing `//` comment.
fn code_only(line: &str) -> String {
    let mut out = String::new();
    let (mut in_str, mut escaped) = (false, false);
    let mut prev = ' ';
    for c in line.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
                out.push('"');
            }
        } else if c == '"' {
            in_str = true;
            out.push('"');
        } else if c == '/' && prev == '/' {
            out.pop();
            break;
        } else {
            out.push(c);
        }
        prev = c;
    }
    out
}

/// Count one source text. A line that names `unsafe_code` (the lint) is not a use.
#[must_use]
pub fn count_source(path: &str, text: &str) -> FileCount {
    let mut c = FileCount {
        path: path.to_owned(),
        code: 0,
        inline_test: 0,
        tests: 0,
        unsafe_uses: 0,
    };
    let mut in_tests = false;
    for line in text.lines().filter(|l| is_code(l)) {
        in_tests |= line.starts_with("#[cfg(test)]");
        if in_tests {
            c.inline_test += 1;
        } else {
            c.code += 1;
        }
        if line.trim_start().starts_with("#[test]") {
            c.tests += 1;
        }
        let code = code_only(line);
        if !code.contains("unsafe_code") && (has_word(&code, "unsafe") || has_word(&code, "extern"))
        {
            c.unsafe_uses += 1;
        }
    }
    c
}

fn rs_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for e in entries {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_dir() {
            out.extend(rs_files(&p)?);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

fn count_dir(crate_dir: &Path, sub: &str) -> Result<Vec<FileCount>, String> {
    rs_files(&crate_dir.join(sub))?
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let rel = p.strip_prefix(crate_dir).unwrap_or(p);
            Ok(count_source(&rel.to_string_lossy(), &text))
        })
        .collect()
}

/// The names of the dependencies in a `Cargo.toml` that are not path
/// dependencies, from `[dependencies]` and `[dev-dependencies]`.
#[must_use]
pub fn external_deps(manifest: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]" || t == "[dev-dependencies]";
            continue;
        }
        if in_deps
            && !t.is_empty()
            && !t.starts_with('#')
            && let Some((name, spec)) = t.split_once('=')
            && !spec.contains("path =")
        {
            out.insert(name.trim().to_owned());
        }
    }
    out
}

fn manifest(dir: &Path) -> Result<String, String> {
    let p = dir.join("Cargo.toml");
    std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Measure one arm crate against the shared baseline.
///
/// # Errors
///
/// Returns a message for an unreadable file or directory.
pub fn measure(root: &Path, arm: &'static str, krate: &str) -> Result<ArmCost, String> {
    let dir = root.join("crates").join(krate);
    let mut baseline = BTreeSet::new();
    for shared in SHARED {
        baseline.extend(external_deps(&manifest(&root.join("crates").join(shared))?));
    }
    let external = external_deps(&manifest(&dir)?);
    let new_deps = external.difference(&baseline).cloned().collect();
    let src = count_dir(&dir, "src")?;
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap_or_default();
    let forbids_unsafe = lib.contains("#![forbid(unsafe_code)]")
        || (workspace.contains("unsafe_code = \"forbid\"")
            && manifest(&dir)?.contains("workspace = true"));
    let fuzz_targets = rs_files(&dir.join("fuzz")).map_or(0, |v| v.len());
    Ok(ArmCost {
        arm,
        src,
        tests: count_dir(&dir, "tests")?,
        external_deps: external,
        new_deps,
        forbids_unsafe,
        fuzz_targets,
    })
}

/// Lines added between the base and the measured commit, by `git diff --numstat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// The base the diff starts from.
    pub base: String,
    /// Lines added in each arm crate's `.rs` files, in [`ARMS`] order.
    pub arms: Vec<usize>,
    /// Lines added in the shared crates' `.rs` files. Neither arm owns them.
    pub shared: usize,
    /// Lines added in `conformance/vectors`.
    pub vectors: usize,
}

/// The conformance vectors in the tree, which gate both arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vectors {
    /// `(file, vectors)` per suite file.
    pub files: Vec<(String, usize)>,
}

impl Vectors {
    /// All vectors.
    #[must_use]
    pub fn total(&self) -> usize {
        self.files.iter().map(|(_, n)| n).sum()
    }
}

/// The result of a cost run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    /// One entry per arm.
    pub arms: Vec<ArmCost>,
    /// The diff against the base.
    pub added: Added,
    /// The conformance vectors.
    pub vectors: Vectors,
}

/// Parse `git diff --numstat` output into lines added per area.
///
/// # Errors
///
/// Returns a message for a line that is not `added<TAB>deleted<TAB>path`.
pub fn parse_numstat(base: &str, numstat: &str) -> Result<Added, String> {
    let mut added = Added {
        base: base.to_owned(),
        arms: vec![0; ARMS.len()],
        shared: 0,
        vectors: 0,
    };
    for line in numstat.lines().filter(|l| !l.is_empty()) {
        let mut f = line.splitn(3, '\t');
        let (Some(n), Some(_), Some(path)) = (f.next(), f.next(), f.next()) else {
            return Err(format!("bad numstat line {line:?}"));
        };
        // A binary file shows `-`.
        let n: usize = n.parse().unwrap_or(0);
        let ext = Path::new(path).extension().and_then(|e| e.to_str());
        if ext == Some("rs") {
            if let Some(i) = ARMS
                .iter()
                .position(|(_, k)| path.starts_with(&format!("crates/{k}/")))
            {
                added.arms[i] += n;
            } else if SHARED
                .iter()
                .any(|k| path.starts_with(&format!("crates/{k}/")))
            {
                added.shared += n;
            }
        } else if path.starts_with("conformance/vectors/") && ext == Some("json") {
            added.vectors += n;
        }
    }
    Ok(added)
}

/// Count the vectors in every suite file under `conformance/vectors`.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed file.
pub fn count_vectors(root: &Path) -> Result<Vectors, String> {
    let dir = root.join("conformance/vectors");
    let mut files = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for p in paths {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let doc: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        let n = [
            "vectors",
            "detector_vectors",
            "track0_vectors",
            "escaped_text_vectors",
        ]
        .iter()
        .map(|k| doc[k].as_array().map_or(0, Vec::len))
        .sum();
        let name = p
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        files.push((name, n));
    }
    Ok(Vectors { files })
}

fn git_numstat(root: &Path, base: &str, commit: &str) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(["diff", "--numstat", base, commit])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git diff {base} {commit} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl CostReport {
    /// The summary text.
    #[must_use]
    pub fn summary(&self, header: &HeaderInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "commit={} harness={} corpus={} session_matrix={} sinks={}",
            header.commit, header.harness, header.corpus, header.session_matrix, header.sinks
        );
        let _ = writeln!(out, "2.8 cost is reported only; it is not gated");
        let _ = writeln!(out, "diff base={}", self.added.base);
        for (a, n) in self.arms.iter().zip(&self.added.arms) {
            let _ = writeln!(out, "{}: lines_added_vs_base={n}", a.arm);
        }
        let _ = writeln!(
            out,
            "shared crates: lines_added_vs_base={} (belong to neither arm); conformance vectors: lines_added_vs_base={}",
            self.added.shared, self.added.vectors
        );
        let _ = writeln!(
            out,
            "conformance vectors={} {:?} (gate both arms)",
            self.vectors.total(),
            self.vectors.files
        );
        for a in &self.arms {
            let _ = writeln!(
                out,
                "{}: src_lines={} test_lines={} test_fns={} new_deps={:?} unsafe_uses={} forbids_unsafe={} fuzz_targets={}",
                a.arm,
                a.src_lines(),
                a.test_lines(),
                a.test_fns(),
                a.new_deps,
                a.unsafe_uses(),
                a.forbids_unsafe,
                a.fuzz_targets
            );
        }
        for a in &self.arms {
            for f in a.src.iter().chain(&a.tests) {
                let _ = writeln!(
                    out,
                    "{} {} code={} inline_test={} test_fns={} unsafe_uses={}",
                    a.arm, f.path, f.code, f.inline_test, f.tests, f.unsafe_uses
                );
            }
        }
        out
    }
}

/// Measure both arms under `root` and write the header-only run log to `log`.
///
/// # Errors
///
/// Returns a message for an unreadable file or a failed write.
pub fn run_cost<W: Write>(
    root: &Path,
    base: &str,
    commit: &str,
    log: W,
) -> Result<(CostReport, HeaderInfo), String> {
    let effects = EffectLog::new(SinkTable::none());
    let header = HeaderInfo {
        commit: commit.to_owned(),
        features: "arm-a arm-b".to_owned(),
        harness: HARNESS_VERSION.to_owned(),
        corpus: "none".to_owned(),
        session_matrix: "none".to_owned(),
        sinks: effects.table_source().to_owned(),
    };
    let rec = Recorder::new(log, &header).map_err(|e| e.to_string())?;
    rec.finish().map_err(|e| e.to_string())?;
    let arms = ARMS
        .iter()
        .map(|(arm, krate)| measure(root, arm, krate))
        .collect::<Result<Vec<_>, _>>()?;
    let added = parse_numstat(base, &git_numstat(root, base, commit)?)?;
    let vectors = count_vectors(root)?;
    Ok((
        CostReport {
            arms,
            added,
            vectors,
        },
        header,
    ))
}
