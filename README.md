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
- **Decision:** on the owner's decision, v0 uses Arm B (Track 0 rich everywhere, with the sealed upgrade); multiplexer panes keep the sealed relay, and Arm A is kept as the evidence record. See [the decision record](docs/phase-0.5/decision.md) for the evidence and what is not measured.
- **Notifications:** v0 notifications are specified in [Notifications](spec/v0/trust.md#trust.notifications). OSC 9, 777 and 99 are intercepted before any emulator sees them. Unsigned output draws nothing by default, or a rate-limited toast with an opt-in, and never an OS notification. Only a sealed frame with the `notifications` capability can raise an OS notification, and only behind a setting that is off by default.
- **Phase 1 progress (Arm B):** the work has started on the chosen design. What is in the repository, and what is not:
  - **Window:** `lumen-term` runs a program behind the Arm B layer on the upstream VT core and draws the result through its own renderer. The trust margin, the editor and the background strip are drawn by the terminal, not by program output. A strip for an older prompt cycle is drawn where that cycle's output ended, collapsed to a row with the count, so program output stays in the flow. A window needs the `window` feature; the default build is headless. Sealed notifications draw toasts in the window's notification area. Unsigned output draws no toast there, because the terminal cannot yet show a layer local. [Trust](spec/v0/trust.md) defines what the margin shows.
  - **Screenshot mode:** `lumen-term --snapshot` renders the real binary headless. The pixel corpus counts changed pixels in the margin and the editor against a baseline taken twice (criteria [5.9](docs/phase-0.5/comparison-criteria.md)). That is a count over the terminal's drawing, in-process and through the real binary's headless snapshot, not a capture of an operating-system window.
  - **Image decoder:** a separate worker process decodes PNG, JPEG and WebP inside a platform sandbox (seccomp on Linux, Seatbelt on macOS), with a host-side runner that bounds and checks its reply. A sandbox test suite covers both platforms; the clean macOS virtual machine proof is pending. The memory limit is the worker's own heap cap on macOS, because the kernel refuses a memory limit there; Linux also sets an address-space limit. The decoder is not yet connected to the terminal: no image frame is parsed, reassembled or uploaded, so images are not shown. A fuzz campaign tool exists; no fuzzing gate run has been done, and the gate is not met. The image-path identity and the rule that unlocks images are in the repository, but nothing calls the rule yet, so images are not shown.
  - **Image manager:** the rules for images are in the repository: reservations against a per-session budget, a queue with per-producer and total in-flight limits, degradation to plain text after a crash or an over-budget image, and a texture budget checked before upload. Nothing in the terminal calls the manager yet, so no image is shown.
  - **Grid parser isolation:** inside a grid block, the string escapes DCS, APC, PM and SOS are dropped, and C1 controls and invalid UTF-8 are removed before the VT parser sees them. OSC is passed only for a short allowlist. A deterministic split-position test feeds every introducer variant at every read boundary; it is not a randomised fuzzer.
  - **Notifications:** the notification rules, the toast drawing and an operating-system notification backend (macOS, Linux and Windows programs, started with the text as data) are in the repository. The backend is off unless `os_notifications` is set, a clean-VM proof on Linux showed a real desktop notification banner from the backend (notify-send through a notification daemon, with markup shown as literal text). A notification from a real program in the window has not been shown yet, because no layer can yet be shown local (see [Notifications](spec/v0/trust.md#trust.notifications)). The `os_text` setting is off by default.
  - **Sealed shell (Linux):** a probe for the sealing conditions and a key-holder process shell are in the repository. The holder accepts messages only from the terminal's shell process, by kernel credentials. It can receive the layer's key over a socket pair and zeroizes it when it ends. It asks the kernel to lock the key's memory and records whether that succeeded; it keeps the key even when locking fails. A sender function for the key exists, but nothing in the terminal calls it yet, so no key is delivered in production. The holder answers no seal or open request, and nothing in the terminal calls the sealing decision or starts a holder, so no layer seals today. macOS and other systems are grid-only by design.
  - **Not claimed:** no release, no gate result for any of the above, and no operating-system window captures.
- **Latest run:** the final comparison rerun at main `d8994bbcbaacda7a94104c07c55722a6f1cedec4`, on a clean Linux virtual machine that was destroyed after the run. The repository's own comparison script ran every mode, and every mode exited as expected. The results are the numbers of that run, and each is stated with its caveat:
  - **Tests:** 287 per-test lines, all passing.
  - **Red-team corpora (2.2):** rt-1, rt-2.1, rt-3 and rt-4 together, 95 cases, give 0 violations for each arm. This checks byte-pipeline output, not what a terminal draws. 330 sealed frames opened on each sealed arm; they were sealed under public test keys, so this exercises the upgrade path only.
  - **Authority leakage (2.4):** zero on the sealed layer for Arm B and for Arm A, with all six sinks reached. That zero comes from a test model of a terminal, not a terminal, and the clipboard and link confirmations are simulated because the harness has no user. Arm A on an unsealed layer is not measured.
  - **Reach (2.1):** 14 of 14 sessions for both arms, on the item-kind oracle rather than screenshots. The macOS rows are modelled on the Linux machine.
  - **Flood (2.6):** a 600 second flood stayed within budget for every arm (500 of 500 blocks drawn on Arm B and on Arm A sealed, none on Arm A unsealed, one degrade notice each). Peak memory (the kernel's high-water mark for the driver process) was 4 MiB. The longest call into the layer was 6.7 to 9.4 ms, a proxy for a UI stall and not a measured stall.
  - **Render log (2.6):** the flood's pipeline log gives the longest gap between returns from the layer as 14, 16 and 17 ms, a proxy only. It is not a drawn-frame gap, and it still exits as not measured.
  - **Parity (2.9):** 0 channels accept more than a PTY write could, for the three channels the spec declares. This is a declaration-level result: none of the three has code in the repository yet.
  - **Multiplexer placement (section 7), pane corpus pc-2, 20 cases, model level:** 0 violations under both placements. Under placement A, 3 of 3 stale-generation containers were dropped, 5 of 5 replays were refused, and 0 of 6 forged pane-written frames opened. Placement B draws no per-pane containers in the model, so its drop count is 0 of 0. Its 3 of 3 refused replays and 0 of 6 opened forged frames come from unauthenticated bytes being refused, not from a replay or forgery check, and say nothing about an attacker who holds genuine frames.
  - **Cost (2.8), lines added against base `5549359`:** Arm A +1481, Arm B +1340. Reported only.
  - **Results bundle:** the raw logs are referenced by hash only, sha256 `2d474869a1662c9b112048bcbbae26351d0882138879dbe5ebba016196740b37`.
- **Components in the repository:** the notification rules are specified in [Notifications](spec/v0/trust.md#trust.notifications). OSC 9, 777 and 99 are intercepted before any emulator sees them. Unsigned output draws nothing by default, or a rate-limited toast with an opt-in, and never an OS notification. The OS notification setting is `os_notifications` (off by default), and the text setting is `os_text`. The unsigned toast opt-in is `allow_unsigned`. A headless block model, `lumen-term`, runs both arms without a window.
- **Not measured yet:** the human spoofing study (2.2a), the simulated-reader proxy (2.2b) and trust-margin pixels (2.3) as an operating-system window count, which needs a window capture on each platform (the drawing-code count is above); decoder robustness (2.5), where the fuzz tooling exists but no gate run has been done ([plan](docs/phase-0.5/decoder-fuzz-plan.md)); the UI stall, frame rate and lying-decoder memory of 2.6, which need a terminal that draws and a decoder; the red-team review (2.7); parity beyond the declaration level; and the section 7 gates for key reuse on reattach, the replayed mark, trust-margin pixels in panes, key custody and pane reach. No outcome under the decision rule is possible until these are measured. The macOS gate experiments are not yet proven on macOS.

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
