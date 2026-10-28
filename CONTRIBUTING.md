# Contributing

Read [`REVIEW_POLICY.md`](REVIEW_POLICY.md) for how a pull request is classified, proven and reviewed. Report security issues privately, as described in [`SECURITY.md`](SECURITY.md).

## Checks

Before opening a PR, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo deny check
```

## Rules of the repository

- Code is licensed `MIT OR Apache-2.0`; the spec is CC-BY-4.0. Copyright (c) Moonbase 2090 LLC.
- Every crate root has `#![forbid(unsafe_code)]`.
- The experiment features `arm-a` and `arm-b` are off by default and never enabled by default.
- The upstream VT core and renderer are used as a pinned git dependency (MIT). Prefer the pinned dependency over copying files; any copied code keeps the upstream copyright notice and MIT licence text. Do not depend on the upstream multiplexer.
- Demos and proofs show real output only. Never fabricate, mock up or illustrate results.
- Commit messages and PR text carry no personal data, local paths, machine names or internal tracker IDs.
