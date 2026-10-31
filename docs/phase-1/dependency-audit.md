# Phase 1 dependency audit gate

What the gate checks, what it does not, and where the unsafe and FFI boundary sits. The gate has two machine checks that run on every pull request (`supply-chain` in `.github/workflows/rust.yml`) and one document, this one, that records the manual part.

## The two checks

1. **`cargo vet --locked`.** Every third-party crate in the lockfile is covered by an audit in `supply-chain/audits.toml` or by an **exemption** in `supply-chain/config.toml`. A new or updated dependency fails the check until someone adds one of the two.
   - **What the numbers mean.** At the first run all 252 third-party packages are exempted and none is audited. **An exemption is not an audit**: it records that the crate was already in the tree when the gate started and has not been reviewed by us. The exemption list is the backlog, not evidence. Importing other projects' public audit lists would cover about 40 of them, but those files carry reviewers' email addresses, which the repository's text rules do not allow; that is a later decision.
   - **The version policy.** `lumen-term` and `portable-pty` carry `audit-as-crates-io` so that the tool treats them as third-party code to be covered. The other Prismattyc crates the build links are not in the tool's set (they are git dependencies of the same owner), so they are covered by the manual review below and by the pin check, not by this tool.
2. **`scripts/unsafe-report.py check`.** For each root crate on the three paths below, the unsafe-code counter runs over the whole dependency tree for the Linux target and the per-package totals (every unsafe item in the source files, not the counter's host-dependent used/unused split) is compared with `docs/phase-1/unsafe-report.json`. Any difference fails: a new dependency with unsafe code, a bumped version, a crate that stops forbidding unsafe, a change in count. After looking at each line, `scripts/unsafe-report.py write` regenerates the file. The counts are the tool's, not a review: they say how much unsafe code there is, and say nothing about whether it is sound.

## The paths

| Path | Root crates | What is on it |
|---|---|---|
| Frame | `lumen-detector` | The split-safe frame detector that runs ahead of the VT parser. No dependencies (the VT parser crate is a dev-dependency used by its tests). `#![forbid(unsafe_code)]`; the report shows 0 unsafe items. |
| Key | `lumen-seal`, `lumen-sealed` | The seal decision and the sealed channel (authenticated frames). `lumen-sealed` forbids unsafe code and the report shows 0 unsafe items in it; its dependencies are the RustCrypto and `getrandom` family, 24 packages, 14 with unsafe code (the usual SIMD, `zeroize`, `cpufeatures` and `getrandom` code). `lumen-seal` denies unsafe code and allows it in one module. |
| Parser | `lumen-decoder`, `lumen-term` | The image decoder worker (`png`, `image-webp`, `zune-jpeg`, and their `unsafe`-using helpers) and the terminal that links the upstream VT core. 125 packages for `lumen-term`, 78 with unsafe code, most of them operating-system and graphics bindings. |

## Our own unsafe and FFI boundary

Unsafe code is confined to named modules; each crate that has any denies it at the crate root and allows it in those modules with a written reason.

| Crate | Where unsafe is allowed | What it wraps |
|---|---|---|
| `lumen-decoder` | `ffi_alloc.rs`, `ffi_process.rs`, `ffi_sandbox.rs` | The capped allocator, process control (resource limits, signals, descriptors), and the sandbox (seccomp on Linux, the sandbox profile on macOS). The decoding module has no unsafe code and there is no C library in the worker. |
| `lumen-seal` | `ffi_linux.rs` | The namespace-parent ioctl and the pid file descriptor reads of the seal decision. Linux only. |
| `lumen-holder` | `ffi_linux.rs`, `spawn.rs` | The system calls the holder and its spawner need (one wrapper per call, each `unsafe` block says why it is sound), and the spawner. |
| `lumen-gates` | `ffi_cred.rs`, `ffi_mem.rs` | Socket-credential primitives for gate G1 and memory-locking primitives for gate G6. |

Every other workspace crate forbids unsafe code, through `#![forbid(unsafe_code)]` or the workspace lint `unsafe_code = "forbid"`. The four crates above set their own `deny` in their manifests, so adding unsafe code to any crate is a visible change to a manifest or a crate root.

## The reused VT core

The terminal links five crates from the upstream VT core repository at the pinned revision `ca7b9f8d6541`: `prismattyc-core`, `prismattyc-emulator`, `prismattyc-render` (through `lumen-term`), `prismattyc-protocol`, and the vendored `portable-pty`. Reviewed at that revision:

| Crate | Unsafe in what the terminal builds | Review |
|---|---|---|
| `prismattyc-core` | None. The 12 items the counter reports are in `examples/scroll_allocations.rs`, an example that is not linked. | Searched the crate for `unsafe`: only that example. |
| `prismattyc-protocol` | None. | Searched the crate for `unsafe`: no matches. |
| `prismattyc-render` | None. | Searched the crate for `unsafe`: no matches. |
| `prismattyc-emulator` | 2 production sites. `graphics/file_transport.rs`: `libc::geteuid()` for the owner check of a file offered by an image upload (no pointers). `lib.rs`: `libc::fcntl(fd, F_GETFD)` on a descriptor the code owns, to refuse a child descriptor that is not close-on-exec; the code carries a `SAFETY` comment. The other 25 sites in the file are in `#[cfg(test)]` code (test pipes, `std::env::remove_var`). | Both production sites read. |
| `portable-pty` (vendored) | About 50 sites in 6 files, mostly the PTY plumbing: `openpty` and `from_raw_fd` for the master and slave, `ttyname_r`, `tcgetattr`, the window-size ioctl, `tcgetpgrp`, `fcntl` for close-on-exec, `kill(SIGHUP)`, `strsignal`, and `getpwuid` for the default shell and home directory. The Windows files (`win/`) are not built on the supported platforms. | Classified by the system call each site makes. **Not read line by line**; this is the open item below. |

Because the VT core is first-party for Lumen (same owner, same review policy), its unsafe code is a review obligation here rather than something the tool can waive. The pin is the control: `Cargo.toml` pins the revision, `deny.toml` allows only that repository as a git source, and a change of the revision changes the report (the package versions and counts), which fails the check until this table is looked at again.

## What this does not claim

- That the 252 exempted dependencies are audited. They are not.
- That any unsafe code in the report is sound. The report counts.
- That the `portable-pty` sites are reviewed beyond their call names, or that the `zune-jpeg`, `png`, `image-webp` and RustCrypto code has been reviewed by us: the decoder's protection against bugs in them is the process sandbox and the fuzz campaign (`decoder-fuzz-campaign.md`), not an audit.

## Open items

- Read the `portable-pty` unsafe sites line by line and record the result in the table above.
- Turn the exemptions for the crates on the three paths into audits, starting with the decoder's libraries and the sealed channel's cryptographic crates, and shrink the exemption list as that is done.
