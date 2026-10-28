# Review policy

This policy applies to every pull request to this repository, whoever wrote it.

## 1. Classify every PR

**Trunk**: the change touches shared code that other parts depend on:
- the shared crates (frame codec, detector, parsers, corpus, render glue) and any public API (`pub` items, crate `src/lib.rs` re-exports)
- the spec under `spec/`
- CI, build, dependency and licence configuration (`.github/`, `Cargo.toml`, `Cargo.lock`, `deny.toml`)
- this policy and `CONTRIBUTING.md`

**Leaf**: everything else, such as arm-specific code behind a feature, an example, a self-contained test or documentation outside the paths above.

A PR that is partly trunk is trunk. When unsure, call it trunk. Label PRs `trunk` or `leaf`.

## 2. Proof

The PR body has a **Proof** section with real evidence: test output, a CI run link, a capture for visual changes, or a before/after for behavior changes. "Tested locally" without output is not proof. Demos and proofs show real output only; nothing is mocked up or illustrated.

## 3. Independent peer review

- Every PR gets a review from someone other than the author.
- The reviewer names the exact tip commit they reviewed and answers **SHIP** or **CHANGES**. A new commit needs a new review at the new tip.
- A SHIP carries over a clean rebase when the PR's own diff is unchanged (`git range-diff` or `git patch-id --stable` shows no difference).
- Trunk PRs are also built and exercised by the reviewer, not just read. Every finding is fixed or explicitly waived in the PR thread.
- Docs-only PRs may merge on green checks with no review.

## 4. Required checks

All required status checks on `main` must pass before merging: fmt, clippy with `-D warnings`, tests, and cargo-deny.

## 5. Feature gating

New behavior ships behind a Cargo feature that is off by default. The experiment arms (`arm-a`, `arm-b`) stay off by default. Turning anything on by default is its own trunk PR.

## 6. Unsafe code

Every crate root has `#![forbid(unsafe_code)]`, except a module explicitly named for FFI (`ffi_*`), which the PR must justify.

## 7. Tests

Every test must fail if the behavior it names broke. Reject tests that assert a value against itself, compute the expected value with the code under test, assert what a mock of the unit under test returns, only check that nothing panicked, or still pass when the implementation is replaced with a stub.

## 8. Merging

- No admin overrides and no force-pushes to `main`.
- Commits carry no private details: no personal names, paths, hostnames or addresses.
- Security issues are reported privately (see `SECURITY.md`), never in a public issue, PR or commit message.
- Releases and tags need owner approval.

## Local verification

Every behavior-changing PR must include proof that the change was built and exercised in a clean local VM of the relevant OS, and shown working: the commands run plus their output. Docs-only and CI-only PRs are exempt. Only an explicit owner override waives this requirement.
