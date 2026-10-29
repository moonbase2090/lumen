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
- **Notifications:** v0 notifications are specified in [Notifications](spec/v0/trust.md#trust.notifications). OSC 9, 777 and 99 are intercepted before any emulator sees them. Unsigned output draws nothing by default, or a rate-limited toast with an opt-in, and never an OS notification. Only a sealed frame with the `notifications` capability can raise an OS notification, and only behind a setting that is off by default.
- **Latest run:** the comparison rerun at main `e27667310b15016c2aa891b1f33d0d5a9750c441`, on a clean Linux virtual machine (arm64, Ubuntu 24.04, an unprivileged user, built with `--locked`, full history), which was destroyed after the run. Every step exited as expected (0 for a measured mode, 3 for a mode with nothing to measure). The raw logs are in a results bundle with sha256 `7f4c99f934a804c846649dfd87e1db37698dd8c7da95e5a1903ae5ea205c4b7a`, referenced by hash and not committed.
  - **Tests:** 276 of 276 per-test lines present and passing.
  - **Red-team corpora (2.2):** rt-1 (15 cases), rt-2 (9), rt-3 (41) and rt-4 (30) give 0 violations for each arm. This checks byte-pipeline output, not what a terminal draws. 261 sealed frames from rt-2 opened on each sealed arm; they were sealed under public test keys, so this exercises the upgrade path only.
  - **Authority leakage (2.4):** zero on rt-4 for Arm B and for Arm A on a sealed layer. All six sinks were reached, and no unsealed-origin input reached any of them. Arm A on an unsealed layer opens no sealed frame, so it is not measured. rt-3 on its own reaches only the notification and grant sinks and finds no leak there, so it is not a verdict by itself. **What a zero means:** the sinks are a test model of a terminal, not a terminal, and the clipboard and link confirmations are simulated because the harness has no user. A zero says that, over these corpora, nothing unsigned reached a sink in that model. It does not say a built terminal has no leak.
  - **Reach (2.1):** 14 of 14 sessions for both arms, on the item-kind oracle rather than screenshots. The macOS rows are modelled on the Linux machine.
  - **Flood (2.6):** a 600 second flood stayed within budget for every arm (500 of 500 blocks drawn on Arm B and on Arm A sealed, none on Arm A unsealed, one degrade notice each). Peak memory was 3 MiB, a sampled value, and the longest call into the layer was about 6.4 to 6.7 ms, a proxy for a UI stall and not a measured one.
  - **Multiplexer placement (section 7), pane corpus pc-1, 18 cases, model level:** 0 violations under both placements. Under placement A the defences show: stale-generation frames dropped 3 of 3, replays refused 2 of 2, and 0 of 4 forged pane-written frames opened. Under placement B the model draws no per-pane containers, so those counters are 0 of 0: zero violations, but no positive evidence. Key reuse, unmarked replay, trust-margin pixels, key custody and pane reach are not measured.
  - **Cost (2.8), lines added against base `5549359`:** Arm A +1481, Arm B +1340. Reported only.
- **Not measured yet:** the human spoofing study (2.2a), the simulated-reader proxy (2.2b) and trust-margin pixels (2.3), which wait for a terminal that draws (Phase 1); decoder robustness (2.5), which has no decoder to fuzz; side-channel parity (2.9) as a test of an implementation, because no channel has code yet (a declaration-level count is available); the UI stall, frame rate and lying-decoder parts of 2.6, which need a UI; and the red-team review (2.7). No outcome under the decision rule is possible until these are measured. The macOS gate experiments are not yet proven on macOS.

### Running the comparison

The evidence driver is a test-only crate, `lumen-evidence`. Each mode writes `run.jsonl` and `summary.txt` to its output directory:

```sh
# Replay a red-team corpus through one arm (b, a-sealed or a-unsealed).
# rt-1 is docs/phase-0.5/corpus; rt-2 (sealed frames) is docs/phase-0.5/corpus-rt-2;
# rt-3 (notification cases) is docs/phase-0.5/corpus-rt-3; rt-4 (sealed frames of every
# type) is docs/phase-0.5/corpus-rt-4.
cargo run -p lumen-evidence -- --corpus docs/phase-0.5/corpus --commit "$(git rev-parse HEAD)" --out out/rt1 --arm b

# Reach over the 14-session matrix (b or a)
cargo run -p lumen-evidence -- --matrix docs/phase-0.5/session-matrix.json --commit "$(git rev-parse HEAD)" --out out/reach --arm b

# Flood for N seconds (2.6)
cargo run -p lumen-evidence -- --flood --seconds 600 --commit "$(git rev-parse HEAD)" --out out/flood --arm b

# Code added per arm against a base commit (2.8)
cargo run -p lumen-evidence -- --cost --root . --base <base-sha> --commit "$(git rev-parse HEAD)" --out out/cost

# Declared channels against the PTY baseline (2.9)
cargo run -p lumen-evidence -- --parity --channels docs/phase-0.5/channels.json --commit "$(git rev-parse HEAD)" --out out/parity

# Authority leakage (2.4): give --corpus more than once to run corpora as one, so one
# verdict covers them. rt-3 is the OSC notification cases and rt-4 is real sealed
# frames. The report lists the simulated user actions the run used.
cargo run -p lumen-evidence -- --corpus docs/phase-0.5/corpus --corpus docs/phase-0.5/corpus-rt-2 --corpus docs/phase-0.5/corpus-rt-3 --corpus docs/phase-0.5/corpus-rt-4 --commit "$(git rev-parse HEAD)" --out out/authority --arm b

# Decoder fuzzing ledger against the 2.5 threshold (not measured without a ledger)
cargo run -p lumen-evidence -- --decoder --ledger <ledger.json> --commit "$(git rev-parse HEAD)" --out out/decoder

# A terminal's render log against the UI stall, frame rate and memory limits of 2.6
cargo run -p lumen-evidence -- --render-log --log <render-log.json> --commit "$(git rev-parse HEAD)" --out out/render
```

`scripts/run-comparison.sh --commit "$(git rev-parse HEAD)" --out out/all --flood-seconds 600` runs all of the modes below in turn and keeps every log.

The modes for 2.5, 2.6 and 2.9 exit 3, not 0, when they measured nothing, so a script cannot read an empty run as a pass. The placement comparison has a pane corpus format (section 7.5 of the criteria) and test-only models of the multiplexer and the outer terminal in the `lumen-panes` crate; they draw nothing and keep a log of the rectangles they would draw.

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
