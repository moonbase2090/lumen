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

/// Which build identity: the gate covers both ([images.unlock] in the spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The decoder: what runs in the sandboxed worker.
    Decoder,
    /// The terminal's image path: what the terminal does around the worker.
    ImagePath,
}

/// Worker side, relative to `crates/lumen-decoder/src`: decoding, the sandbox,
/// the heap cap, the wire format, the limits and the worker binary.
const DECODER_SOURCES: &[&str] = &[
    "decode.rs",
    "ffi_sandbox.rs",
    "ffi_alloc.rs",
    "limits.rs",
    "protocol.rs",
    "bin/lumen-decoder-worker.rs",
];

/// Terminal side, relative to `crates/lumen-decoder/src`: running a worker,
/// bounding what it may write, validating the reply, and the crate root. The
/// wire format and the limits are shared with the worker, so they are in both.
///
/// **This is the host half only.** The image-frame parser, multi-frame
/// reassembly and the upload step do not exist yet; their sources join this
/// list when they do, and until then an identity of this list does not cover
/// the whole image path.
const IMAGE_PATH_SOURCES: &[&str] = &[
    "runner.rs",
    "ffi_process.rs",
    "limits.rs",
    "protocol.rs",
    "unlock.rs",
    "images.rs",
    "lib.rs",
];

fn sources(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Decoder => DECODER_SOURCES,
        Kind::ImagePath => IMAGE_PATH_SOURCES,
    }
}

/// The files that make up one build identity: its sources, the crate manifest
/// and the workspace lockfile (the dependency closure, with versions and hashes).
///
/// # Errors
/// If a listed file is missing, so that renaming a source cannot quietly shrink
/// an identity.
pub fn files(root: &Path, kind: Kind) -> io::Result<Vec<PathBuf>> {
    let src = root.join("crates/lumen-decoder/src");
    let mut files: Vec<PathBuf> = sources(kind).iter().map(|f| src.join(f)).collect();
    files.push(root.join("crates/lumen-decoder/Cargo.toml"));
    files.push(root.join("Cargo.lock"));
    for f in &files {
        if !f.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a file", f.display()),
            ));
        }
    }
    files.sort();
    Ok(files)
}

/// Source files of the crate that are in neither identity (the test-only worker
/// is left out on purpose). A new file must be added to a list, or an edit to it
/// would not change any identity.
///
/// # Errors
/// If a directory cannot be read.
pub fn uncovered(root: &Path) -> io::Result<Vec<PathBuf>> {
    let src = root.join("crates/lumen-decoder/src");
    let mut all = Vec::new();
    walk(&src, &mut all)?;
    let covered: Vec<PathBuf> = [Kind::Decoder, Kind::ImagePath]
        .iter()
        .flat_map(|k| sources(*k).iter().map(|f| src.join(f)))
        .collect();
    all.retain(|p| {
        !covered.contains(p)
            && p.file_name()
                .is_none_or(|n| n != "lumen-decoder-hostile.rs")
    });
    all.sort();
    Ok(all)
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
