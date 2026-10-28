# Phase 0.5 comparison criteria

This document fixes the criteria for the Phase 0.5 comparison before any arm code exists. It is a docs-only change. Changing a threshold, a measurement method or the decision rule after an arm has been measured requires a new revision of this file, reviewed in its own PR, and the results must say which revision they were judged against.

Normative terms follow [spec/v0](../../spec/v0/_index.md). The arms are described in terms of the [unverified output (Track 0)](../../spec/v0/trust.md) rules, [sealed layers](../../spec/v0/sealed-layers.md), the [multiplexer](../../spec/v0/multiplexers.md) rules and the [conformance gates](../../spec/v0/conformance.md).

## 1. The two arms

Both arms are built in this repository behind Cargo features `arm-a` and `arm-b`, both off by default. They share the same renderer and the same red-team corpus. Each arm is built from a single commit, and every result records that commit SHA.

**Arm A: sealed-only rich.** Rich blocks render only on a sealed layer. Track 0 output on an unsealed layer renders as escaped text. The spec only states this for the alternate-screen and multiplexer cases ([trust](../../spec/v0/trust.md), [multiplexers](../../spec/v0/multiplexers.md#mux.alt-screen)); applying it to every unsealed layer under arm A is a proposal of this document, not a spec rule. Multiplexer behaviour follows [multiplexers](../../spec/v0/multiplexers.md#mux.lumen-aware). Where rich panes are placed is compared separately in section 7.

**Arm B: decoupled.** Track 0 renders on every layer, subject to the Track 0 rules. The sealed upgrade applies on the default model: a layer that meets the [default sealing requirements](../../spec/v0/sealed-layers.md#sealed.requirements) gets sealed authority on top of Track 0. This is the expected default.

Both arms run every test in this document. An arm that cannot run a test reports that test as "not measured", never as a pass.

## 2. Metrics

Each metric has a unit, a measurement method, the harness that produces it and a pass threshold. "Harness" means the comparison harness, which is described in its own PR and must be merged before any result is recorded. Every harness run writes a raw log. The log is committed or attached to the result (section 6).

### 2.1 Reach

- **Unit:** percent of sessions in the session matrix where the intended rich rendering is shown for the sealed or Track 0 content in that session.
- **Method:** a fixed session matrix covering local Linux and macOS, SSH, serial, nested shells, a plain multiplexer, a Lumen-aware multiplexer and an alternate-screen application. Each session plays a scripted run of commands and output. A session counts as reached when the screenshot-diff oracle matches the expected rendering for every block in it.
- **Harness:** comparison harness, session replay mode.
- **Threshold:** arm B must reach at least **80%** of sessions. Arm A's reach is reported but is not gated. This threshold is part of the decision rule in section 4.

### 2.2 Spoof success

- **Unit:** count of forged command structures, exit statuses, producer identities or verified badges that the terminal shows for output that did not come from a sealed frame, and percent of spoofing attempts that succeed.
- **Method:** run the shared red-team corpus through each arm. For each attempt, record whether the terminal drew any command boundary, exit status, producer identity or verified badge that the output forged.
- **Harness:** comparison harness, corpus mode, with the spoofing user study as a separate protocol (section 2.2a).
- **Threshold:** the sealed authority path has a spoof count of zero in arm A and in arm B. Any non-zero count in arm B is a failure of the decision rule.

### 2.2a Spoofing user study

- **Unit:** error rate, meaning the fraction of trials in which a participant accepts a spoofed command boundary, exit status or verified badge as genuine.
- **Method:** a controlled study with the same trial set for three conditions: arm A, arm B and a plain VT control with no rich rendering. Trials are randomised per participant and the study records each trial's result.
- **Threshold:** arm B's error rate must be no worse than the plain-VT control, and at most **2 percentage points** worse than arm A.

### 2.2b Simulated-reader spoofing evaluation (proxy, veto only)

This is a provisional proxy. It is not the spoofing study in section 2.2a, and it does not settle the spoofing rule.

- **Unit:** per arm, the number of genuine and spoofed rendered prompts that a simulated reader judges to be genuine, and the fraction of spoofed prompts accepted.
- **Method:** a panel of simulated readers, each a fixed persona run against the same rendered prompts. Each reader sees real rendered output only, as screenshots taken from the arm's build. Screenshots are never mocked or edited. Each reader labels every prompt as genuine or spoofed. The panel's accepted-spoof fraction per arm is the result.
- **Harness:** the shared evidence harness, screenshot mode.
- **Use:** veto only. An arm whose accepted-spoof fraction is above the plain-VT control's is out. Passing this check does not decide anything about the spoofing rule.
- **Follow-up:** a human panel of about 20 to 30 people runs the protocol in section 2.2a. Until it has run, the spoofing rule is pending.

### 2.3 Trust-margin pixels

- **Unit:** count of changed pixels in the trust margin and chrome, across the fuzzed Track 0 corpus, versus a baseline screenshot taken with no Track 0 input.
- **Method:** trust-area screenshot and pixel diff. The corpus includes badge glyphs, images of badges, negative offsets, huge tables and reflow. Each screenshot is compared to the baseline with no tolerance.
- **Harness:** comparison harness, pixel-diff mode.
- **Threshold:** **zero** changed pixels in each arm. Any non-zero count in arm B fails the decision rule. A non-zero count in arm A is a failure of arm A's gate and blocks its release, but does not change the decision rule.

### 2.4 Authority leakage

- **Unit:** count of unsealed inputs that cause a clipboard write, notification, link open, grant, exit status or boundary change.
- **Method:** the output-adversary corpus is run against each arm with the terminal's authority-effect log enabled. Each event is recorded with the input that produced it.
- **Harness:** comparison harness, authority-log mode.
- **Threshold:** **zero** events. Any non-zero count in arm B fails the decision rule.

### 2.5 Decoder robustness

- **Unit:** CPU-hours of fuzzing per enabled image format, new crashes in the last 168 CPU-hours, and coverage plateau status.
- **Method:** coverage-guided fuzzing of each enabled format under ASan and UBSan, with seed corpora that include inputs from past CVEs. Hangs over 2 s, memory use over 512 MiB and sanitizer reports count as crashes.
- **Harness:** the decoder fuzzing harness, run against the decoder build identity and the terminal image-path build identity.
- **Threshold:** at least 1000 CPU-hours per format, no new crash in the last 168 CPU-hours, coverage plateau reached, sandbox escape tests passing. This is the [decoder gate](../../spec/v0/conformance.md#gate.decoder). Until it passes, images stay off, and the arm is measured with images off.

### 2.6 Denial of service

- **Unit:** peak resident memory of the terminal under a hostile decoder, sustained frame rate and budget violations under a Track 0 flood, and the longest UI stall.
- **Method:** run a flood of Track 0 frames from an unauthenticated producer and a lying decoder stream, and record peak memory, budget counters and the longest interval without a frame being drawn.
- **Harness:** comparison harness, flood mode, and the decoder memory test.
- **Threshold:** the terminal stays within its budgets and reservations (the [decoder memory gate](../../spec/v0/conformance.md#gate.decoder-memory)). No stall longer than 2 s (a proposed value; the spec's 2 s limit applies to decoder hangs, not UI stalls). Peak memory under the 512 MiB decoder limit. The unauthenticated producer gets no more than its 25% share of each budget.

### 2.7 Red-team findings

- **Unit:** count of open findings by severity, graded against the [threat model](../../spec/v0/threat-model.md), after each arm has been run against the shared corpus.
- **Method:** a separate reviewer runs the red-team review against each arm's build commit. Findings are recorded against the commit, not against the arm name.
- **Harness:** review, not automated. The reviewer's notes are part of the evidence.
- **Threshold:** **zero** open Medium or higher findings in each arm at the commit that is judged. Lows are recorded as accepted residuals with a reason.

### 2.8 Cost

- **Unit:** lines of code added in each arm relative to the shared baseline, new dependencies, new `unsafe` or FFI modules (each justified), and number of new fuzz targets and test vectors required to hold the arm's gates.
- **Method:** a diff measurement at each arm's build commit against the common base, plus a count of the test vectors that gate it.
- **Harness:** comparison harness, cost mode.
- **Threshold:** **reported only**. Cost does not enter the decision rule. It is a tie-breaker only if the decision rule above leaves both arms eligible, and it must be stated in the result if it is used.

### 2.9 Parity

- **Unit:** count of Lumen channels that accept an action a PTY write could not already perform.
- **Method:** the [side-channel parity](../../spec/v0/conformance.md#gate.parity) test: for each channel, enumerate the actions it accepts and check each against the actions a plain PTY writer can already trigger.
- **Harness:** comparison harness, parity mode.
- **Threshold:** **zero** channels that accept more than a PTY write could. Any failure in arm B fails the decision rule.

## 3. Hard gates and preconditions

An arm's feature is off on a target OS until the gate that protects it passes there. A gate failure is not a comparison result. It means the feature is measured with that feature off, and the result says so. The gates below are preconditions for each arm's sealed authority, and they apply on each target OS where that authority is used.

- **Per-message sender credentials on the seal channel (Linux).** The seal channel verifies the sender of each message. Sealed authority is off on Linux until it passes.
- **Memory locking.** Key pages are locked under the default memory-lock limit. If locking fails, no keys are stored and the layer runs as grid plus Track 0 with a notice. See the [sealing gates](../../spec/v0/conformance.md#gate.sealing).
- **Holder death.** When the terminal exits, every key holder exits with it, and the EOF pipe's write end is held by no child, including the PTY child. A failure fails closed to grid plus Track 0 with a notice.
- **Exec and ptrace matrix (Linux, shell integration).** After any exec, the seal fd is gone and no frame is applied. The ptrace and process-tree matrix passes at the Yama scope the default requires. Sealed authority on Linux is off until this passes. macOS remains grid plus Track 0 for shell integration.
- **Track 0 gates.** The [trust margin](../../spec/v0/conformance.md#gate.trust-margin), [no authority from unsealed input](../../spec/v0/conformance.md#gate.no-authority) and [side-channel parity](../../spec/v0/conformance.md#gate.parity) gates must pass for arm B's Track 0 to be on by default.
- **Decoder and decoder memory gates.** Required before images are on in either arm (section 2.5 and 2.6).

## 4. Decision rule

1. Each arm must pass every precondition in section 3 on the OS where its feature is on. An arm that fails a precondition on an OS is measured with that feature off there, and the result says so.
2. **Arm B is chosen** unless one of these holds:
   - the trust-area-pixel count (section 2.3) is non-zero in arm B;
   - the authority-leak count (section 2.4) is non-zero in arm B;
   - arm B fails the spoofing user study (section 2.2a): its error rate is worse than the plain-VT control, or more than 2 percentage points worse than arm A. **Pending human study:** this clause is decided by the human study in section 2.2a, not by the automated metrics or the simulated-reader proxy in section 2.2b. Until the study has run, it is recorded as pending and does not by itself change the outcome. A veto from section 2.2b applies at once.
3. Arm B must also reach **at least 80%** of sessions (section 2.1). If it does not, the rule does not choose arm B. The automated metrics decide this clause.
4. If arm B is not chosen, arm A is the default for the sealed path. This document does not decide the default for Track 0 on unsealed layers. The spec's Track 0 default ([trust](../../spec/v0/trust.md)) would have to change for arm A's rule, and that change is a spec revision, outside this comparison.
5. Failures in arm B on parity (section 2.9), spoof success (section 2.2), DoS (section 2.6) or red-team findings (section 2.7) do not change the choice in steps 2 and 3. They do block acceptance of the result: each must be fixed, or waived in writing, before the result is accepted.

This decision rule restates the phase 0.5 comparison in [spec/v0](../../spec/v0/_index.md). It does not change it.

## 5. What counts as evidence

- Every number in a result comes from a recorded run. Estimates, mock-ups and illustrative figures do not count.
- Each run's raw log is committed to `docs/phase-0.5/evidence/` or attached to the result PR. Logs include the build commit SHA, the OS and version, the harness version and the corpus revision.
- Each result states the build commit SHA it was measured at. A new commit needs a new run.
- Screenshots used for the trust-area pixel diff are committed with their baseline and the diff output.
- A metric that was not run is reported as "not measured" with the reason. It is not reported as a pass.
- Security findings are reported privately through the process in the [security policy](../../SECURITY.md). Results that describe a vulnerability are not written into a public result, commit message or PR text beyond what the public spec already says.

## 6. Results format

A result is a docs PR that adds `docs/phase-0.5/results-<arm>.md` for each arm at a stated commit SHA. It contains:

- the build commit SHA, OS and version, and the harness and corpus revisions;
- a table with one row per metric in section 2, showing unit, value, threshold, pass or fail, and the path to the raw log;
- the decision outcome under section 4, and any gate that was off during the measurement.

## 7. Multiplexer placement comparison

This section compares where rich panes are placed in a Lumen-aware multiplexer. It runs on the same evidence harness and the same corpus as sections 2 to 6, and its results are recorded in the same results format.

### 7.1 The two placements

**Option A: sealed containers (default).** The multiplexer parses each pane's Track 0 itself and re-emits it as sealed pane-container frames, with an opaque pane id, a layout generation and a rectangle. Its keys are scoped to the layout and pane-container frame types only, as described in [multiplexers](../../spec/v0/multiplexers.md#mux.pane-containers). Rich panes are only on platforms where the [multiplexer gate](../../spec/v0/conformance.md#gate.multiplexer) passes.

**Option B: unsealed producer (off by default).** The multiplexer is treated as an ordinary unsealed producer, as the [unsealed multiplexer mode](../../spec/v0/multiplexers.md#mux.unsealed) describes. It gets only the Track 0 rules and the possibly-remote treatment. It draws no per-pane containers, so it has no rich panes.

### 7.2 Metrics

Each metric is a count or percentage over the pane corpus, which covers resize, zoom, layout races, reattach and a fuzzed pane parser. Every run records the build commit SHA and the platform.

- **Cross-pane and hidden-pane drawing.** Unit: count of draws outside a pane's rectangle, and count of draws from a hidden or detached pane. Method: pixel and draw-log check across resize, zoom and layout-race scripts. Threshold: **zero**.
- **Trust-margin pixels in panes.** Unit: count of changed pixels in the outer trust margin and in each per-pane strip, against a baseline with no pane input. Method: the same pixel diff as section 2.3, over the pane corpus. Threshold: **zero**.
- **Stale-generation drop.** Unit: percent of frames with a stale layout generation that are dropped. Method: replay of out-of-generation frames. Threshold: **100%**.
- **Forged frames stay text.** Unit: count of forged sealed-looking frames from a pane that the outer terminal draws as containers. Method: the forgery corpus from the pane. Threshold: **zero** drawn as containers; every forged frame renders as text.
- **Reattach.** Unit: count of reattaches that reuse prior keys, and percent of replayed content without the "replayed" mark. Method: reattach scripts with key-material logging. Threshold: **zero** reused keys; **100%** of replayed content marked.
- **Introducer fuzzing of the pane parser.** Unit: count of frames accepted from bytes without valid sealing, and count of frame bytes that reach the VT parser. Method: every introducer variant at every split position, run against the pane parser. Threshold: **zero** in both counts.
- **Key custody.** Unit: count of keys found in the multiplexer server's memory, environment or socket, and count of keys held by a client not spawned by the terminal. Method: memory and environment scan of the server during a run. Threshold: **zero** in both counts.
- **Pane reach.** Unit: percent of pane sessions in the matrix that show rich panes as intended. Method: screenshot-diff oracle, as section 2.1. Threshold: **reported only**, not gated.

### 7.3 Decision rule

1. Option A is the placement used. Rich panes are enabled on a platform only when every metric in section 7.2 that is gated has zero failures there, and the multiplexer gate passes there.
2. On any platform where Option A's gate does not pass, rich panes stay off. Option B is the state where rich panes are off. Its result is "no rich panes", not a rich-pane result.
3. The placement results do not change the arm decision in section 4.

### 7.4 Preconditions

The Track 0 gates in section 3 and the [alternate-screen rule](../../spec/v0/multiplexers.md#mux.alt-screen), which applies per pane, must pass before any rich pane is measured.
