# Phase 0.5 comparison criteria

This document fixes the criteria for the Phase 0.5 comparison before any arm code exists. It is a docs-only change. Changing a threshold, a measurement method or the decision rule after an arm has been measured requires a new revision of this file, reviewed in its own PR, and the results must say which revision they were judged against.

Normative terms follow [spec/v0](../../spec/v0/_index.md). The arms are described in terms of the [unverified output (Track 0)](../../spec/v0/trust.md) rules, [sealed layers](../../spec/v0/sealed-layers.md), the [multiplexer](../../spec/v0/multiplexers.md) rules and the [conformance gates](../../spec/v0/conformance.md).

## 1. The two arms

Both arms are built in this repository as separate crates, `lumen-arm-a` and `lumen-arm-b`. The Cargo features `arm-a` and `arm-b` of the `lumen` crate, both off by default, select which crate is linked, and the comparison driver picks one with `--arm`. They share the same renderer and the same red-team corpus. Each arm is built from a single commit, and every result records that commit SHA.

**Arm A: sealed-only rich.** Rich blocks render only on a sealed layer. Track 0 output on an unsealed layer renders as escaped text. The spec only states this for the alternate-screen and multiplexer cases ([trust](../../spec/v0/trust.md), [multiplexers](../../spec/v0/multiplexers.md#mux.alt-screen)). Applying it to every unsealed layer under arm A is a proposal of this document, not a spec rule.

**Arm B: decoupled.** Track 0 renders on every layer, subject to the Track 0 rules. The sealed upgrade applies on the default model: a layer that meets the [default sealing requirements](../../spec/v0/sealed-layers.md#sealed.requirements) gets sealed authority on top of Track 0. This is the expected default. The `ssh.track0` default of `rich` ([possibly remote](../../spec/v0/trust.md#trust.possibly-remote)) describes arm B. Arm A's unsealed-layer rule (escaped text, no rich blocks) overrides that default for arm A.

Where rich panes are placed in a Lumen-aware multiplexer is compared separately, in section 7.

Both arms run every test in this document. An arm that cannot run a test reports that test as "not measured", never as a pass.

## 2. Metrics

Each metric has a unit, a measurement method, the harness that produces it and a pass threshold. "Harness" means the comparison harness, which is described in its own PR and must be merged before any result is recorded. Every harness run writes a raw log. The log is committed or attached to the result (section 5).

### 2.1 Reach

- **Unit:** percent of sessions in the session matrix that show the rendering the arm specifies for that session's platform and state. A gate-off state counts as the arm's specified state, so it is not a miss.
- **Session matrix:** fixed at 14 sessions, unweighted. It is seven session types on two platforms (Linux and macOS): local shell, SSH, serial, nested shell, plain multiplexer, Lumen-aware multiplexer and an alternate-screen application. The matrix file, with one scripted run per session, is committed with the harness, and its revision is recorded in each result.
- **Method:** each session plays its scripted run. A session is reached when the screenshot-diff oracle (section 2.3, with the same render conditions) matches the arm's specified rendering for every block in it.
- **Harness:** comparison harness, session replay mode (section 5.2).
- **Threshold:** arm B must reach at least **80%**, which is at least 12 of the 14 sessions. Arm A's reach is reported but is not gated. This threshold is part of the decision rule in section 4.

### 2.2 Spoof success

- **Unit:** count of forged command structures, exit statuses, producer identities or verified badges that the terminal shows for output that did not come from a sealed frame. Also percent of spoofing attempts that succeed, reported only.
- **Method:** run the shared red-team corpus, at a stated corpus revision, through each arm. For each attempt, record whether the terminal drew any command boundary, exit status, producer identity or verified badge that the output forged.
- **Harness:** comparison harness, corpus mode. The spoofing user study is a separate protocol (section 2.2a).
- **Threshold:** a count of **zero** in arm A and in arm B. Any non-zero count in arm B is a failure of the decision rule. The percent is not gated.

### 2.2a Spoofing user study

- **Unit:** error rate, meaning the fraction of trials in which a participant accepts a spoofed command boundary, exit status or verified badge as genuine.
- **Method:** a controlled study with the same trial set for three conditions: arm A, arm B and a plain VT control with no rich rendering.
- **Threshold:** arm B's error rate must be no worse than the plain-VT control, and at most **2 percentage points** worse than arm A.
- **Not yet fixed:** the sample size, the definition of a trial, participant selection and exclusions, and the statistical comparison (point estimate or confidence bound, and which interval) belong to the study protocol. The protocol is a separate docs PR, merged before the study runs. Until that protocol exists, this clause cannot be evaluated.

### 2.2b Simulated-reader spoofing evaluation (proxy, veto only)

This is a provisional proxy. It is not the spoofing study in section 2.2a, and it does not settle the spoofing rule. Its veto is a proposal that needs owner sign-off before it is used (section 4, step 2).

- **Unit:** per arm, the fraction of spoofed rendered prompts that the simulated-reader panel accepts as genuine, and the same fraction for the plain-VT control on the same prompts.
- **Method:** a fixed panel of **10 simulated readers** (proposed), each a distinct, fixed persona, run against the same prompt set at a stated corpus revision. Each reader sees real rendered output only, as screenshots taken from the arm's build with the render conditions in section 2.3. Screenshots are never mocked or edited. Each reader labels every prompt as genuine or spoofed.
- **Harness:** the shared evidence harness, screenshot mode.
- **Use (proposed):** veto only. An arm, A or B, is vetoed if its accepted-spoof fraction is above the plain-VT control's on the same prompts. Passing does not decide anything about the spoofing rule.
- **Follow-up:** the human study in section 2.2a is the decision input for the spoofing rule.

### 2.3 Trust-margin pixels

- **Unit:** count of changed pixels in the trust margin and chrome, across the fuzzed Track 0 corpus, versus a baseline screenshot taken with no Track 0 input.
- **Render conditions:** baseline and run use the same platform and build class, the same pinned font set, a scale factor of 2, a light theme, cursor blink off and animations off. Baselines are kept per platform. Before any run counts, the baseline is captured twice on the same build, and the two captures must match exactly. If they do not, the oracle is invalid for that platform.
- **Trust-margin region:** a list of exact pixel rectangles per layout, committed with the corpus revision. "Changed pixels" is counted inside those rectangles only.
- **Method:** trust-area screenshot and pixel diff. The corpus includes badge glyphs, images of badges, negative offsets, huge tables and reflow. Each screenshot is compared to its baseline with no tolerance.
- **Harness:** comparison harness, pixel-diff mode.
- **Threshold:** **zero** changed pixels in each arm. Any non-zero count in arm B fails the decision rule. A non-zero count in arm A is a failure of arm A's gate and blocks its release, but does not change the decision rule.

### 2.4 Authority leakage

- **Unit:** count of unsealed inputs that cause a clipboard write, notification, link open, grant, exit status or boundary change.
- **Method:** the output-adversary corpus, at a stated corpus revision, is run against each arm with the terminal's authority-effect log enabled. Each event is recorded with the input that produced it. A count under this criterion counts only once the rt-3 cases, including the OS notification cases, actually reach the sinks (section 5.7); until they do, the verdict stays not measured.
- **Harness:** comparison harness, authority-log mode (section 5.7).
- **Threshold:** **zero** events. Any non-zero count in arm B fails the decision rule.

### 2.5 Decoder robustness

- **Unit:** CPU-hours of fuzzing per enabled image format, new crashes in the last 168 CPU-hours, and coverage plateau status.
- **Method:** coverage-guided fuzzing of each enabled format under ASan and UBSan, with seed corpora that include inputs from past CVEs. Hangs over 2 s, memory use over 512 MiB and sanitizer reports count as crashes.
- **Plateau (proposed):** no new coverage edges over the last 168 CPU-hours.
- **Sandbox escape tests:** the test set, named by revision in the harness, must pass in full.
- **Harness:** the decoder fuzzing harness, run against the decoder build identity and the terminal image-path build identity.
- **Threshold:** at least 1000 CPU-hours per format, no new crash in the last 168 CPU-hours, plateau reached, and the named sandbox escape tests passing. This is the [decoder gate](../../spec/v0/conformance.md#gate.decoder). Until it passes, images stay off, and the arm is measured with images off.

### 2.6 Denial of service

- **Unit:** peak resident memory of the terminal under a hostile decoder, budget violations under a Track 0 flood, and the longest UI stall. Sustained frame rate is reported only.
- **Flood parameters (proposed):** one run per platform, lasting 10 minutes, with an unauthenticated producer writing Track 0 at the maximum rate it can sustain. The total bytes and frame count are recorded in the log.
- **Method:** run the flood alongside a lying decoder stream, and record peak memory, budget counters, the longest interval without a frame being drawn and the sustained frame rate. The next comparison rerun uses the full 10-minute flood, not the 30 s run of the second comparison.
- **Harness:** comparison harness, flood mode (section 5.3), and the decoder memory test.
- **Threshold:** the terminal stays within its budgets and reservations (the [decoder memory gate](../../spec/v0/conformance.md#gate.decoder-memory)). Peak memory stays under the 512 MiB decoder limit. The unauthenticated producer gets no more than its 25% share of each budget. No UI stall longer than 2 s (proposed; the spec's 2 s limit applies to decoder hangs, not UI stalls). Frame rate is not gated.

### 2.7 Red-team findings

- **Unit:** count of open findings by severity, after each arm has been run against the shared corpus.
- **Method:** a reviewer independent of the arm's author runs the red-team review against each arm's build commit. Findings are recorded against the commit, not against the arm name. The severity rubric is fixed in the review record at the corpus revision and cited in the result. The public spec states that a fixed rubric was used but does not publish it.
- **Harness:** review, not automated. The reviewer's notes are part of the evidence.
- **Threshold:** **zero** open Medium or higher findings in each arm at the commit that is judged. Lows are recorded as accepted residuals with a reason.

### 2.8 Cost

- **Unit:** lines of code added in each arm relative to the shared baseline, new dependencies, new `unsafe` or FFI modules (each justified), and number of new fuzz targets and test vectors required to hold the arm's gates.
- **Method:** a diff measurement at each arm's build commit against the common base (section 5.4), plus a count of the test vectors that gate it. The arms are separate crates over shared crates, so lines added to the shared crates are reported apart and belong to neither arm; the conformance vectors gate both arms and are counted once.
- **Harness:** comparison harness, cost mode (section 5.4).
- **Threshold:** **reported only**. Cost does not enter the decision rule. It is a tie-breaker only if the decision rule above leaves both arms eligible, and it must be stated in the result if it is used.

### 2.9 Parity

- **Unit:** count of Lumen channels that accept an action a PTY write could not already perform.
- **Method:** the [side-channel parity](../../spec/v0/conformance.md#gate.parity) test: for each channel, enumerate the actions it accepts and check each against the actions a plain PTY writer can already trigger.
- **Harness:** comparison harness, parity mode (section 5.5).
- **Threshold:** **zero** channels that accept more than a PTY write could. Any failure in arm B fails the decision rule.

## 3. Hard gates and preconditions

An arm's rich rendering is off on a target OS until the gate that protects it passes there. A gate failure is not a comparison result. It means the arm is measured with that rendering off, and the result says so.

The preconditions below come from the sealing and Track 0 gates in the [conformance gates](../../spec/v0/conformance.md). They apply to the default sealing path that both arms use.

- **Memory locking.** Key pages are resident and excluded from dumps. If locking fails, no keys are stored and the layer runs as grid plus Track 0 with a notice. See the [sealing gates](../../spec/v0/conformance.md#gate.sealing).
- **Holder death.** When a holder dies, its layer fails closed to grid plus Track 0 with a notice. See the [sealing gates](../../spec/v0/conformance.md#gate.sealing).
- **Track 0 gates.** The [trust margin](../../spec/v0/conformance.md#gate.trust-margin), [no authority from unsealed input](../../spec/v0/conformance.md#gate.no-authority) and [side-channel parity](../../spec/v0/conformance.md#gate.parity) gates must pass for Track 0 to be on by default (section 1, arm B).
- **Decoder and decoder memory gates.** Required before images are on in either arm (sections 2.5 and 2.6).

Hardened-mode checks are not preconditions here. The [hardened mode](../../spec/v0/hardened-mode.md) gates, including its same-uid Linux checks, apply only to a hardened-mode run. Hardened mode is off by default and is not part of this comparison.

## 4. Decision rule

1. Each arm must pass every precondition in section 3 on the OS where its rich rendering is on. An arm that fails a precondition on an OS is measured with that rendering off there, and the result says so.
2. **Arm B is chosen** unless one of these holds:
   - the trust-area-pixel count (section 2.3) is non-zero in arm B;
   - the authority-leak count (section 2.4) is non-zero in arm B;
   - arm B fails the spoofing user study (section 2.2a): its error rate is worse than the plain-VT control, or more than 2 percentage points worse than arm A. **Pending human study:** this clause is decided by the human study, not by the automated metrics. Until the study has run, it is recorded as pending and does not by itself change the outcome.
   - **Proposed, pending owner sign-off:** the section 2.2b veto applies to either arm. A vetoed arm is out. This is not part of the confirmed phase 0.5 rule until the owner signs it off.
3. Arm B must also reach **at least 80%** of sessions (section 2.1, at least 12 of 14). If it does not, the rule does not choose arm B. The automated metrics decide this clause.
4. If arm B is not chosen, arm A is the default for the sealed path. This document does not decide the default for Track 0 on unsealed layers. The spec's Track 0 default ([trust](../../spec/v0/trust.md)) would have to change for arm A's rule, and that change is a spec revision, outside this comparison.
5. Failures in arm B on parity (section 2.9), spoof success (section 2.2), DoS (section 2.6) or red-team findings (section 2.7) do not change the choice in steps 2 and 3. They do block acceptance of the result: each must be fixed, or waived in writing, before the result is accepted.

Steps 2 and 3 restate the phase 0.5 comparison in [spec/v0](../../spec/v0/_index.md). Only the 2.2b bullet in step 2 is new, and it is marked as a proposal above.

## 5. What counts as evidence

- Every number in a result comes from a recorded run. Estimates, mock-ups and illustrative figures do not count.
- Each run's raw log is committed to `docs/phase-0.5/evidence/` or attached to the result PR. Logs include the build commit SHA, the OS and version, the harness version, the session matrix revision and the corpus revision.
- Each result states the build commit SHA it was measured at. A new commit needs a new run.
- Screenshots used for the trust-area pixel diff are committed with their baseline, the trust-margin rectangles and the diff output.
- A metric that was not run is reported as "not measured" with the reason. It is not reported as a pass.
- A test run is recorded with `scripts/capture-tests.sh OUT_FILE`, which writes cargo's output straight to the file and fails (exit 3) if the file does not hold one `test NAME ... ok|FAILED|ignored` line for every test the `test result:` lines count. A test pass is reported from that log, not from an exit status alone.
- Security findings are reported privately through the process in the [security policy](../../SECURITY.md). Results that describe a vulnerability are not written into a public result, commit message or PR text beyond what the public spec already says.

### 5.1 Corpus format

The shared red-team and replay corpus is a directory of JSON files, read in name order. Each file is `{"revision": "...", "cases": [...]}`, and every file in the directory must give the same revision. A case has:

- `id` and `category`, both strings. Ids are unique across the corpus.
- `reads`: the PTY reads in order. Each read is `{"hex": "..."}` or `{"fill": "<hex byte>", "count": n}`, the same segment encoding as `conformance/vectors`, with an optional `t_ms` (default 0).
- `context`: optional `alt_screen`, `tmux_or_screen`, `allow_unsigned` and `ssh` booleans (default false), and `container` (default true).
- `forbid`: optional item kinds that must not appear. Each entry must be one of the item kinds the layer reports. An unknown kind fails the load with the case id, so a typo cannot pass silently.

- `expect`: optional positive expectation, `{"b": [...], "a": [...]}`, the item kinds Arm B and Arm A must produce, in order. Adjacent `output` items and adjacent `text` items are each joined first, because a read boundary can split them. Kinds are checked like `forbid`, and a key other than `b` and `a` fails the load. The red-team corpus does not use `expect`; the corpus replay ignores it. The reach mode (2.1) requires it on every session of the matrix.
- `sinks`: optional `{"notification_attempts": n, "denied_notifications": m, "toasts": t}`, the counts the case must produce (5.7): OS notifications that reached the sink, notifications denied or dropped, and notifications drawn as a toast. A count that differs makes the case a violation, and a key that is left out is not checked. An unknown key or a value that is not a whole number fails the load with the case id. `context` also takes `allow_unsigned` and `ssh` (both default false), the policy and layer facts for the case.

The format is defined in `crates/lumen-evidence/src/lib.rs`. A change to it is a change to the corpus revision and needs both arms' agreement.

### 5.2 Session matrix format and reach mode

The session matrix is one file, `docs/phase-0.5/session-matrix.json`: `{"revision": "...", "sessions": [...]}`. A session is a case as in 5.1 with three more fields: `session` (one of `local-shell`, `ssh`, `serial`, `nested-shell`, `plain-multiplexer`, `lumen-multiplexer`, `alt-screen-app`), `platform` (`linux` or `macos`) and `sealed` (whether the layer's sealing requirements hold in that session; only Arm A reads it). It must hold exactly one session for each type on each platform, 14 in all, and every session must have an `expect` for both arms. A file that does not fit fails the load with the reason, so a missing or doubled cell cannot be counted. The format is defined in `crates/lumen-evidence/src/matrix.rs`; a change to it is a change to the matrix revision.

`lumen-evidence --matrix FILE --commit SHA --out DIR [--arm b|a]` plays each session through the arm and writes `run.jsonl` and `summary.txt`. A session is reached when the item kinds the arm produced equal the session's `expect`. This is the item-level oracle: it checks what the byte pipeline hands to the renderer. It is not the screenshot-diff oracle that 2.1 names, so the summary says `oracle=item-kinds (screenshot oracle not run)` and a result must not present the count as the screenshot-based reach. The scripted bytes name the platform, but the platform does not change the expected item kinds, so the Linux and macOS rows of the committed matrix have equal expectations at this level; they differ once the screenshot mode runs.

The run log header (format 2) records `commit`, `features`, `harness` (the harness version), `corpus` (the corpus revision, or `none`), `session_matrix` (the matrix revision, or `none`) and `sinks`. A reach run has `corpus` `none`; a corpus replay has `session_matrix` `none`.

The first matrix (`sm-1`) has one Track 0 paragraph after a shell command in each session. It has no sealed frames yet, so Arm A's sealed rendering is not exercised by it; the sealed-frame sessions arrive with the sealed-frame corpus. An SSH image reach case changes the existing SSH session's `expect` once the arms' Track 0 parser has an image kind. It does not add a fifteenth session cell, so the 14-cell denominator and the 80% threshold stay as they are.

### 5.3 Flood mode

`lumen-evidence --flood --seconds N --commit SHA --out DIR [--arm b|a-sealed|a-unsealed]` runs the byte-pipeline part of 2.6. One command's output receives, for N seconds and as fast as the layer takes it, 64 KiB reads rotating through valid Track 0 paragraph frames, headers with a wrong tag, bare introducers and ordinary output. Nothing in it is authenticated. The log has the header of 5.2 (`corpus` and `session_matrix` are `none`), a `flood` case marker and one line per second with that second's input bytes and, per item kind, the payload length produced.

The summary reports:

- `budget`: blocks drawn (in the container or the strip) and their bytes against the unauthenticated share (a quarter of 2000 frames and 8 MiB per command), and the number of degrade notices, which must be at most one. The run exits 1 on a violation.
- `peak_rss_mib`: peak resident memory of the driver process against the 512 MiB limit, with its source. On Linux that is the kernel's high-water mark; elsewhere it is the largest of samples taken every 100 ms, so it can miss a short peak.
- `longest_feed_ms`: the longest single call into the layer. It is a proxy for how long a caller would be held up. It is not the UI stall of 2.6.

Three things are reported as `not measured`, with the reason, because the driver has no UI or decoder: the longest UI stall, the sustained frame rate and the lying-decoder stream. The 10-minute duration of 2.6 is the caller's choice of N. A shorter run is a shorter run and its result says so.

### 5.4 Cost mode

`lumen-evidence --cost --root DIR --base REF --commit SHA --out DIR` measures the repository at DIR, which must be checked out at the stated commit and contain REF. The arms are separate crates over the same shared crates (detector, Track 0 parser, sealed layer). The run reports:

- `lines_added_vs_base`: the lines added between REF (the common base) and the measured commit, from `git diff --numstat`, for the `.rs` files of each arm crate. The same count is given for the shared crates and for `conformance/vectors`; edits there belong to neither arm and are not added to an arm's figure. The result states which REF it used.
- Per arm, from the tree: `src_lines` and `test_lines` (code lines, meaning lines that are neither blank nor start with `//`; block comments are not recognised, so a line inside one counts; lines after a top-level `#[cfg(test)]` in a source file are test lines, as is everything under `tests`), and `test_fns` (`#[test]` attributes).
- `new_deps`: external (non-path) dependencies, with dev-dependencies, that none of the shared crates already has. Path dependencies are not counted.
- `unsafe_uses` and `forbids_unsafe`: lines with the word `unsafe` or `extern` outside a trailing `//` comment and outside a string literal (raw strings and block comments are not recognised), and whether the crate root or the workspace lints forbid unsafe code. A non-zero count needs its module named and justified in the result.
- `fuzz_targets`: `.rs` files under the crate's `fuzz` directory.
- `conformance vectors`: the number of vectors in each file under `conformance/vectors`. They gate both arms equally, so one count is given for both and none is attributed to an arm; each arm's own test functions are the per-arm count of tests that hold its gates.

The summary, with its per-file counts, is the raw log; `run.jsonl` holds only the header. Cost is reported and not gated (2.8).

**Cost base.** The base for comparison runs is commit `5549359`. At that commit only the `lumen` and `lumen-detector` crates exist, and no arm crate does. It is an ancestor of main. Every cost result states the REF it used, and results are compared like for like only when they use this base.

### 5.5 Parity mode

`lumen-evidence --parity --channels FILE --commit SHA --out DIR` counts the Lumen channels that accept an action a PTY write could not already perform (2.9). The harness cannot discover channels, so the arm's author declares them in the channel catalogue, `docs/phase-0.5/channels.json`: `{"revision": "...", "channels": [...]}`, where a channel is `{"id": "...", "actions": [...]}` and an action is `{"id": "...", "pty_equivalent": "..."}`. `pty_equivalent` says in a sentence which PTY write already performs the same action, or is `null` when none does. A `null` makes its channel one that accepts more than a PTY write could. Channel ids are unique, and action ids are unique within a channel.

The count is only as complete as the catalogue, so a result must say that the catalogue was checked against the channel code at the measured commit, and by whom. The run exits 0 only for a measured zero, 1 when the count is not zero and 3 when no channel is declared; 2 is a usage or load error. A catalogue with no channel is reported as `2.9=not measured` and exits 3, not 0, so a gate script cannot read it as a pass: a count over nothing says nothing about the gate. The committed catalogue (`ch-1`) declares no channel, because no channel (Track B, the seal channel) has code in the arms yet. `run.jsonl` holds only the header.

### 5.6 Decoder mode

`lumen-evidence --decoder [--ledger FILE] --commit SHA --out DIR` judges a decoder fuzzing ledger against the threshold of 2.5. The driver does not fuzz: there is no image decoder in this repository and images are off. With no ledger the summary says `2.5=not measured`. The run exits 0 when the gate holds, 1 when it does not and 3 when nothing was measured: no ledger, or a ledger that enables no format. Exit 0 is therefore never printed for a run that measured nothing.

The ledger is a JSON object with `revision`, `decoder_build` and `image_path_build` (the gate covers both identities), one entry per enabled format and the sandbox escape result:

- a format has `name`, `cpu_hours`, `crashes` (each `at_cpu_hour` and `kind`) and `coverage` (each `at_cpu_hour` and the cumulative `edges`);
- `sandbox_escape` has the test set `revision` and the `passed` and `total` counts.

A format passes when it has at least 1000 CPU-hours, no crash after the last 168 CPU-hours' start (a crash exactly 168 CPU-hours before the end is older), and a plateau: the most coverage seen over the whole run was already seen at or before that point. A coverage reading from before the window is needed, so a ledger without one has no plateau. The gate passes when every format passes and the sandbox escape set passed in full and is not empty. A ledger that enables no format is `not measured`.

### 5.7 Terminal sinks (authority leakage)

The corpus replay (5.1) passes the ordinary output each arm's layer passes on to the terminal model in the `lumen-sinks` crate, and reports each effect through the harness effect log with the origin of the input. Ordinary output is always unsealed. The summary's `2.4=` line and the per-sink `exercised`, `unsealed` and `foreign` counts are printed as the harness reports them, and the log header names `lumen-sinks` as the source of the sink table.

**Notification guard.** The model puts a guard in front of the upstream emulator. It removes OSC 9, OSC 777 and OSC 99 from the bytes before the emulator sees them, whether the output is grid output, Track 0 output or orphan output, and whatever the multiplexer or alternate-screen state. The introducers are `ESC ]`, the raw 8-bit C1 byte 0x9D outside a UTF-8 sequence, and its UTF-8 form C2 9D. The number is read as digits, so `009` is OSC 9. A sequence may end with BEL, `ESC \` or the 8-bit ST; CAN, SUB or another escape sequence also ends it. The guard keeps its state across reads.

By default a removed notification is denied: it draws nothing, and the denial is counted (`denied_notifications`). Unsigned output never raises an OS notification. With the policy `allow-unsigned`, on a local layer that is not an SSH session or a multiplexer pane (`tmux_or_screen` set), and for grid output only (not orphan output, where no command container is open), a removed notification is drawn as a terminal toast (`toasts`), at most one per layer per second and four in ten seconds on the read timestamps; the rest are dropped and counted as denied. The emulator is still read as a tripwire, so a notification that gets past the guard counts as an unsealed attempt at the sink (`notification_attempts`), and every rt-3 case expects zero of those.

**What is still not measured.** The emulator has no code for clipboard writes, link opens, grants, exit status or command boundaries; those belong to the terminal window and to the sealed frame types, which the transport section of the spec numbers. They are not registered, so they show as never exercised and the verdict is `not measured`, never zero. A zero needs every sink exercised, with a sealed-origin attempt at each; a sink that was never reached proves nothing. A result must print the per-sink lines and may report only the sinks it exercised. Sealed notifications (type 0x22 with the `notifications` grant) are not in the corpus yet.

The notification cases are in `docs/phase-0.5/corpus-rt-3` (revision `rt-3`), each with its `sinks` counts.

### 5.8 Render-log mode

`lumen-evidence --render-log [--log FILE] --commit SHA --out DIR` judges the parts of 2.6 that need a UI and a decoder: the longest UI stall, the sustained frame rate and the peak memory under a lying decoder stream. The driver has neither, so a terminal writes the log while the flood of 5.3 and the lying decoder stream run against it, and this mode applies the limits.

The log is a JSON object with `revision`, `build` (the terminal build identity), `duration_ms`, `frames_ms` (the time of every frame drawn, in order) and, optionally, `lying_decoder` with `decoder_build`, `streams` (a whole number: the count of lying streams played, not a list) and `peak_rss_mib`. The longest stall is the longest gap between consecutive points of 0, each frame time and `duration_ms`, so the wait for the first frame and the wait after the last count. It must be at most 2000 ms. The frame rate is frames over `duration_ms`, reported and not gated. Peak memory must stay under 512 MiB, and a log with no lying stream played does not measure it.

The run exits 0 when every part is within its limit, 1 when a measured part is over, and 3 when a part is not measured: no log, no frames, or no lying decoder run. A failure takes precedence over a missing part. 2 is a usage or load error, and it includes a `streams` or `peak_rss_mib` that is not a whole number, so a list of streams is refused, not read as zero.

## 6. Results format

A result is a docs PR that adds `docs/phase-0.5/results-<arm>.md` for each arm at a stated commit SHA. It contains:

- the build commit SHA, OS and version, and the harness, session matrix and corpus revisions;
- a table with one row per metric in section 2, showing unit, value, threshold, pass or fail, and the path to the raw log;
- the decision outcome under section 4, and any gate that was off during the measurement.

## 7. Multiplexer placement comparison

This section compares where rich panes are placed in a Lumen-aware multiplexer. It runs on the same evidence harness and the same corpus as sections 2 to 6, and its results are recorded in the same results format.

### 7.1 The two placements

**Option A: sealed containers (default).** The multiplexer parses each pane's Track 0 itself and re-emits it as sealed pane-container frames, with an opaque pane id, a layout generation and a rectangle. Its keys are scoped to the layout and pane-container frame types only, as described in [multiplexers](../../spec/v0/multiplexers.md#mux.pane-containers). Rich panes are only on platforms where the [multiplexer gate](../../spec/v0/conformance.md#gate.multiplexer) passes.

**Option B: unsealed producer.** The multiplexer is treated as an ordinary unsealed producer, as the [unsealed multiplexer mode](../../spec/v0/multiplexers.md#mux.unsealed) describes. It gets only the Track 0 rules and the possibly-remote treatment. It draws no per-pane containers, so it has no rich panes. The spec makes this mode off by default.

### 7.2 Metrics

Each metric is a count or percentage over the pane corpus, which covers resize, zoom, layout races, reattach and a fuzzed pane parser. Every run records the build commit SHA and the platform.

- **Cross-pane and hidden-pane drawing.** Unit: count of draws outside a pane's rectangle, and count of draws from a hidden or detached pane. Method: pixel and draw-log check across resize, zoom and layout-race scripts. Threshold: **zero**.
- **Trust-margin pixels in panes.** Unit: count of changed pixels in the outer trust margin and in each per-pane strip, against a baseline with no pane input. Method: the same pixel diff and render conditions as section 2.3, over the pane corpus. Threshold: **zero**.
- **Stale-generation drop.** Unit: percent of frames with a stale layout generation that are dropped. Method: replay of out-of-generation frames. Threshold: **100%**.
- **Forged frames stay text.** Unit: count of forged sealed-looking frames from a pane that the outer terminal draws as containers. Method: the forgery corpus from the pane. Threshold: **zero** drawn as containers; every forged frame renders as text.
- **Reattach.** Unit: count of reattaches that reuse prior keys, and percent of replayed content without the "replayed" mark. Method: reattach scripts with key-material logging. Threshold: **zero** reused keys; **100%** of replayed content marked.
- **Introducer fuzzing of the pane parser.** Unit: count of frames accepted from bytes without valid sealing, and count of frame bytes that reach the VT parser. Method: every introducer variant at every split position, run against the pane parser. Threshold: **zero** in both counts.
- **Key custody.** Unit: count of keys found in the multiplexer server's memory, environment or socket, and count of keys held by a client not spawned by the terminal. Method: memory and environment scan of the server during a run. Threshold: **zero** in both counts.
- **Pane reach.** Unit: percent of pane sessions in the matrix that show rich panes as intended. Method: screenshot-diff oracle with the section 2.3 render conditions. Threshold: **reported only**, not gated.

### 7.3 Placement decision

1. **Option A is selected on a platform** when every gated metric in section 7.2 has zero failures there and the multiplexer gate passes there. Rich panes are on only in that case.
2. **Option B is selected on a platform** where Option A's gate does not pass. The outcome there is "no rich panes", and the result records it as the Option B outcome.
3. The placement outcome does not change the arm decision in section 4.

This is a selection rule: a result can select Option B on a platform, so it is a comparison. Option A is the default because the spec makes it the default, not because the rule prefers it regardless of results.

### 7.4 Preconditions

The Track 0 gates in section 3 and the [alternate-screen rule](../../spec/v0/multiplexers.md#mux.alt-screen), which applies per pane, must pass before any rich pane is measured.

### 7.5 Pane corpus format

The pane corpus is a directory of JSON files read in name order, with the rules of section 5.1: every file is `{"revision": "...", "cases": [...]}`, every file gives the same revision, and ids are unique across the corpus. A case has `id`, `category`, `screen` (`cols` and `rows` at the start), `script` and an optional `forbid`.

A script is a list of steps, each an object with an `op`:

- `layout`: `generation` and `panes`, a list of `{id, x, y, w, h, visible}` in cells. `visible` defaults to true. `x` and `y` may be negative and `w` and `h` may be zero, because a malformed rectangle is a test input. A layout may not name a pane id twice.
- `output`: `pane` and the bytes the pane writes, as `hex` or as `fill` and `count` like the reads of 5.1, with an optional `t_ms`. The pane id may be one that no layout names. A forged pane-container frame is written this way.
- `resize`: `cols` and `rows`. `zoom`: `pane`, or no `pane` to clear the zoom. `detach` and `reattach`.

A script says what the multiplexer is told and what its panes write. It does not say how a layout or pane-container frame is encoded, so a case does not depend on the payload layout of those frames.

`forbid` lists outcomes that must not happen. The kinds cover five of the 7.2 metrics: `draw_outside_rect` and `draw_hidden_pane` (cross-pane and hidden-pane drawing), `stale_generation_drawn` (stale-generation drop), `forged_container_drawn` (forged frames stay text), and `key_reuse` and `unmarked_replay` (reattach). Three metrics have no kind and are measured another way: trust-margin pixels in panes need the screenshot and pixel diff of 2.3, key custody needs a memory and environment scan of the multiplexer server during a run, and pane reach is reported only and needs the screenshot oracle. Introducer fuzzing of the pane parser uses this corpus as input (a pane writes the variants at every split position with `output`), but its two counts, frames accepted without valid sealing and frame bytes reaching the VT parser, are read from the parser's counters, not from `forbid`. An unknown kind fails the load with the case id, so a typo cannot pass silently.

The format is defined in `crates/lumen-evidence/src/panes.rs`. A change to it is a change to the pane corpus revision and needs both arms' agreement.
