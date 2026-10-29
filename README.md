# Lumen

Lumen is an open protocol for structured terminal content: blocks, text runs, layout and events. **Bright Term** is its Rust reference terminal.

Terminals still render a 1970s character grid. Lumen treats a session as a document. Prose, logs and tool replies get proportional fonts and browser-style layout, while legacy apps keep running untouched.

## Core ideas
- **Blocks, not scrollback.** Each command and its output is a block.
- **Rich blocks.** Opt-in headings, real tables, images, links and collapsible sections.
- **Grid blocks.** A layout element with its own cols x rows, backed by a PTY sized to the block. vim, htop, ssh and existing TUIs run inside, alt-screen apps grow to the viewport and collapse back to a snapshot on exit, and old and new apps share one document.
- **Not backwards compatible by design**, with a native middle path for legacy apps.
- **GPU text** with full shaping, ligatures and emoji, and no web engine.

## Principles
1. **Secure by default.** Untrusted output must never be able to forge protocol frames. The threat model is part of the spec.
2. **Evidence over opinion.** Every design fork is prototyped both ways before v0 picks one.
3. **Developed in public.** Design, spec and code happen in the open.

## Status
Lumen is in **Phase 0.5**: a comparison of two candidate designs before v0 is chosen.

- **Spec:** Lumen Protocol v0, revision 25 (frozen snapshot), in [spec/v0](spec/v0/_index.md). It is a public draft, not a stable release, and breaking changes are expected. Revision 25 takes in-place edits until it is published; after that, changes go in new revisions.
- **Comparison:** Arm A (sealed-only rich) and Arm B (Track 0 everywhere, with the sealed upgrade) are built in this repository as separate crates, `lumen-arm-a` and `lumen-arm-b`. The criteria, the decision rule and the results are in [docs/phase-0.5](docs/phase-0.5/comparison-criteria.md).
- **Latest run:** a second comparison run at main `7529aca`, on a clean Linux VM. It measured parts of 2.1, 2.2 and 2.6 only:
  - reach on the session matrix: 14 of 14 sessions, on the item-kind oracle rather than screenshots;
  - forbidden items over the red-team corpora (rt-1, rt-2): 0 for each arm, which checks byte-pipeline output only, not what a terminal draws;
  - sealed frames: 261 test-key sealed frames from the rt-2 corpus opened on each sealed arm. They were sealed under public test keys, not real keys, so this exercises the upgrade path only;
  - flood: within budget for a 30 s run. Peak memory is a sampled value, and the longest call into the layer is a proxy for a UI stall, not a measured stall.
- **Not measured yet:** the human spoofing study (2.2a), the simulated-reader proxy (2.2b), trust-margin pixels (2.3), authority leakage (2.4), decoder robustness (2.5), the red-team review (2.7), the cost metric (2.8), side-channel parity (2.9) and multiplexer placement (section 7). The cost and parity modes exist, but no result for them is recorded yet. No outcome under the decision rule is possible until these are measured. The macOS gate experiments are not yet proven on macOS.

### Running the comparison

The evidence driver is a test-only crate, `lumen-evidence`. Each mode writes `run.jsonl` and `summary.txt` to its output directory:

```sh
# Replay a red-team corpus through one arm (b, a-sealed or a-unsealed).
# rt-1 is docs/phase-0.5/corpus; rt-2 (sealed frames) is docs/phase-0.5/corpus-rt-2.
cargo run -p lumen-evidence -- --corpus docs/phase-0.5/corpus --commit "$(git rev-parse HEAD)" --out out/rt1 --arm b

# Reach over the 14-session matrix (b or a)
cargo run -p lumen-evidence -- --matrix docs/phase-0.5/session-matrix.json --commit "$(git rev-parse HEAD)" --out out/reach --arm b

# Flood for N seconds (2.6)
cargo run -p lumen-evidence -- --flood --seconds 600 --commit "$(git rev-parse HEAD)" --out out/flood --arm b

# Code added per arm against a base commit (2.8)
cargo run -p lumen-evidence -- --cost --root . --base <base-sha> --commit "$(git rev-parse HEAD)" --out out/cost

# Declared channels against the PTY baseline (2.9)
cargo run -p lumen-evidence -- --parity --channels docs/phase-0.5/channels.json --commit "$(git rev-parse HEAD)" --out out/parity
```

A result states the commit it was measured at, and the criteria say which revision it was judged against. Modes that cannot measure something report it as "not measured" rather than as a pass.

The formal rules are in [docs/phase-0.5/comparison-criteria.md](docs/phase-0.5/comparison-criteria.md); the results files are [results for Arm A](docs/phase-0.5/results-arm-a.md) and [results for Arm B](docs/phase-0.5/results-arm-b.md).

## Projects
- **Lumen**: the protocol specification
- **Bright Term**: Rust reference terminal
- **Lunatui**: the first native client library

## Security
Please don't open public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).

## License
Code is dual-licensed under MIT or Apache-2.0, at your option. The Lumen specification is licensed under CC-BY-4.0.
