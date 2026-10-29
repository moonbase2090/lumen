//! Build identity: a hash of the sources, the compiler, the flags and the
//! dependency closure (the lockfile, with its versions and hashes).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// The files that make up the decoder build: its sources (not the test-only
/// worker), its manifest, and the workspace lockfile.
///
/// # Errors
/// If a directory cannot be read.
pub fn decoder_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(&root.join("crates/lumen-decoder/src"), &mut files)?;
    files.retain(|p| {
        p.file_name()
            .is_none_or(|n| n != "lumen-decoder-hostile.rs")
    });
    files.push(root.join("crates/lumen-decoder/Cargo.toml"));
    files.push(root.join("Cargo.lock"));
    files.sort();
    Ok(files)
}

/// Hashes `files` (path relative to `root`, then contents), then the compiler
/// description and the build flags, as lower-case hex.
///
/// # Errors
/// If a file cannot be read.
pub fn hash(root: &Path, files: &[PathBuf], compiler: &str, flags: &str) -> io::Result<String> {
    let mut h = Sha256::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(f);
        let bytes = fs::read(f)?;
        h.update(rel.to_string_lossy().as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    h.update(b"compiler\0");
    h.update(compiler.as_bytes());
    h.update(b"flags\0");
    h.update(flags.as_bytes());
    Ok(h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    }))
}
