# Results: Arm B (Track 0 everywhere plus the sealed upgrade)

Third comparison run, at main `e27667310b15016c2aa891b1f33d0d5a9750c441`. The earlier runs (at 7529aca and 726cb4a) are kept at the end of this file.

This run measures parts of 2.1, 2.2, 2.4, 2.6 and 2.8, and the model-level counts of 7.2 (reported in [results-placement.md](results-placement.md)). Section 4 of the [criteria](comparison-criteria.md) cannot be applied: 2.2a, 2.3 and 2.9 are not measured.

## What this run does and does not show

- **2.2 checks byte-pipeline output only.** The driver replays corpora through the arm's byte pipeline and counts forbidden items. Nothing is drawn, so it does not show that a terminal would draw no forged boundary, exit status, producer identity or badge.
- **Reach uses the item-kinds oracle, not screenshots.** Criterion 2.1 names the screenshot-diff oracle, so the reach figure is a partial measurement and is not counted as the 2.1 result. The macOS reach rows are modelled by the harness on the Linux VM; nothing ran on macOS.
- **2.4 rests on rt-4.** Only rt-4 exercises all six sinks, so the authority-leak verdict is taken from it, with rt-3 covering the notification and grant path. 2.4 is not measured for the unsealed layer of Arm A, which has no sealed layer to exercise the sinks, and not for rt-1, rt-2 or rt-3 on their own. Clipboard and link confirmations are simulated user actions, listed in the run output.
- **The flood is a byte-pipeline run.** The 600-second flood checks the unauthenticated share of the budget and peak memory of the driver process. The longest single call into the layer is a proxy, not the UI stall; frame rate, UI stall and the lying decoder are not measured.
- **The per-test list is complete this time.** The capture check counted 276 per-test lines and 276 tests, all ok (see Run).
- **The G1, G6 and G7 gates are not met** until the macOS clean-VM proof for the gate experiments is posted. This run does not switch any gate on or off.

## Run

- Build commit: `e27667310b15016c2aa891b1f33d0d5a9750c441` (main)
- Machine: clean Linux VM, arm64, Ubuntu 24.04, rustc 1.99.1, unprivileged user, built with `--locked`, full history cloned so the cost base was available
- Harness: `lumen-evidence-2`. Session matrix: `sm-1`. Corpora: `rt-1` (15 cases), `rt-2` (9), `rt-3` (41), `rt-4` (30) and the pane corpus `pc-1` (18).
- Overall: `scripts/capture-tests.sh --locked --workspace` passed with 276 per-test lines and 276 tests counted, 276 ok, 0 failed, 0 ignored; the release build passed; all 23 evidence runs exited with their expected codes (0 for the measured modes, 3 for parity, decoder and render-log, which are not measured).
- Raw logs: the results tarball with sha256 `7f4c99f934a804c846649dfd87e1db37698dd8c7da95e5a1903ae5ea205c4b7a` holds each run's `run.jsonl`, `summary.txt` and output, the test log (sha256 `848b78a85b888bd2be466a17e94213cc9a699215debdbba480955e51c251afae`), the build log and the checksums, which were verified after extraction. It is referenced by hash only and is not committed.

## Metrics

| Metric | Unit | Value | Threshold | Result | Raw log |
|---|---|---|---|---|---|
| 2.1 Reach | sessions reached of 14 (sm-1) | 14/14 | B at least 12/14; A reported, not gated | partial: item-kinds oracle, not the screenshot oracle the criterion names; macOS rows modelled on Linux. Meets 12/14 on that oracle. | results tarball `7f4c99f934a8…` |
| 2.2 Spoof success, rt-1 | forbidden items over 15 cases | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7f4c99f934a8…` |
| 2.2 Spoof success, rt-2 | forbidden items over 9 cases | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7f4c99f934a8…` |
| 2.2 Spoof success, rt-3 | forbidden items over 41 cases | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7f4c99f934a8…` |
| 2.2 Spoof success, rt-4 | forbidden items over 30 cases | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7f4c99f934a8…` |
| 2.2a Spoofing user study | % | n/a | B at most plain VT, and at most A + 2 pp | not measured (pending): the human study has not been run. | n/a |
| 2.2b Simulated-reader spoofing (proxy, veto only) | % | n/a | veto if above plain VT | not measured: there is no screenshot mode. | n/a |
| 2.3 Trust-margin pixels | px | n/a | 0 | not measured: there is no screenshot or pixel-diff mode. | n/a |
| 2.4 Authority leakage, rt-4 | leak events over 30 cases | 0 (all six sinks exercised: clipboard 1, notification 6, link open 1, grant 17, exit status 1, boundary change 4; unsealed 0, foreign 0) | 0 | zero on the sealed layer; confirmations are simulated | results tarball `7f4c99f934a8…` |
| 2.4 Authority leakage, rt-3 alone | leak events over 41 cases | notification and grant leaks 0 (notification exercised 4, grant 9; unsealed 0, foreign 0) | 0 | not measured as a verdict: clipboard, link open, exit status and boundary change are not exercised there. The 4 cases that expect an OS notification each got one, and the unsigned, SSH, multiplexer, focused and setting-off cases got only toasts or denials | results tarball `7f4c99f934a8…` |
| 2.5 Decoder robustness | CPU-hours per format | n/a | section 2.5 | not measured: no fuzzing ledger and no image decoder (exit 3). | n/a |
| 2.6 Denial of service, flood budget | budget violations over a 600 s flood | within, 500 of 500 blocks drawn, 1 degrade notice | within the 25% unauthenticated share | pass for the byte pipeline over 600 s, the duration the criterion proposes. Input: 14.10 GB in 215,082 reads | results tarball `7f4c99f934a8…` |
| 2.6 Denial of service, peak memory | MiB (kernel high-water mark) | 3 MiB | under 512 MiB | pass for the driver process, not a terminal | results tarball `7f4c99f934a8…` |
| 2.6 Denial of service, longest feed | ms | 6.437 ms | UI stall under 2 s (proposed) | reported only: the longest single call into the layer is a proxy, not the UI stall | results tarball `7f4c99f934a8…` |
| 2.6 Denial of service, UI stall, frame rate, lying decoder | n/a | n/a | section 2.6 | not measured: render-log mode ran with no log (exit 3); there is no UI or decoder. | n/a |
| 2.7 Red-team findings | open findings at Medium or above | n/a | 0 | not measured: this is a manual independent review. | n/a |
| 2.8 Cost | lines added vs base 5549359 | 1340 | none (reported only) | reported: source 381 lines, test 721 lines, 29 test functions, no new external dependencies, 0 `unsafe` uses, forbids unsafe, 0 fuzz targets. Shared crates 2484 lines and conformance vectors 986 belong to neither arm | results tarball `7f4c99f934a8…` |
| 2.9 Parity | channels | n/a | 0 | not measured: the channel catalogue declares 0 channels (exit 3). | n/a |
| 7.x Multiplexer placement | model-level counts of 7.2 | see [results-placement.md](results-placement.md) | section 7.3 | model level only; key reuse, replayed mark, trust-margin pixels, key custody and pane reach are not measured | results tarball `7f4c99f934a8…` |

## Decision outcome

No outcome under section 4. Criteria 2.2a, 2.3 and 2.9 are not measured, and 2.1 is only partly measured, so the rule cannot be evaluated. Gates G1, G6 and G7 are not met until the macOS clean-VM proof is posted. The multiplexer placement comparison has no outcome either; see [results-placement.md](results-placement.md).

## Earlier runs (kept)

### Second run, at main 7529aca

Second comparison run, at main `7529aca649448252111ca79f3c55a38f7d11aadc`. The first run (at 726cb4a) is kept at the end of this file.

This run measures parts of 2.1, 2.2 and 2.6 and nothing else. Section 4 of the [criteria](comparison-criteria.md) cannot be applied: 2.2a, 2.3 and 2.9 are not measured.

#### What this run does and does not show

- **2.2 checks byte-pipeline output only.** The driver replays corpora through the arm's byte pipeline and counts forbidden items. Nothing is drawn, so it does not show that a terminal would draw no forged boundary, exit status, producer identity or badge.
- **Reach uses the item-kinds oracle, not screenshots.** Criterion 2.1 names the screenshot-diff oracle, so the reach figure below is a partial measurement and is not counted as the 2.1 result.
- **The macOS reach rows are modelled on Linux.** The harness models the macOS sessions on the Linux VM. Nothing ran on macOS.
- **The test pass rests on exit status.** The workspace tests passed because the run exited 0 under `pipefail`. The per-test lines were cut off by the console capture, so they are not part of this result. The capture is to be fixed for the next run.
- **The G1, G6 and G7 gates are not met** until the macOS clean-VM proof for the gate experiments is posted. This run does not switch any gate on or off.

#### Run

- Build commit: `7529aca649448252111ca79f3c55a38f7d11aadc` (main, which includes pull requests 35 to 39)
- Machine: clean Linux VM, arm64, Ubuntu 24.04, unprivileged user, built with `--locked`
- Harness: `lumen-evidence-2`. Session matrix: `sm-1`. Corpora: `rt-1` (15 cases) and `rt-2` (9 sealed-frame cases).
- Overall: `cargo test --locked --workspace` and the release build passed, and all 11 evidence runs exited 0.
- Raw logs: the results tarball with sha256 `7b46bd7fb24ed5f1deaf78b587fc3ccb1ba0487b5639a956323b17224debbe7b` holds the 11 `run.jsonl` and `summary.txt` pairs. It is referenced by hash only and is not committed.

#### Metrics

| Metric | Unit | Value | Threshold | Result | Raw log |
|---|---|---|---|---|---|
| 2.1 Reach | sessions reached of 14 (sm-1) | 14/14 | B at least 12/14; A reported, not gated | partial: item-kinds oracle, not the screenshot oracle the criterion names; macOS rows modelled on Linux. Meets 12/14 on that oracle. | results tarball `7b46bd7fb24e…` |
| 2.2 Spoof success | forbidden items over rt-1 | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7b46bd7fb24e…` |
| 2.2 Spoof success | forbidden items over rt-2 | 0 | 0 | 0 in every run. Byte pipeline only, nothing is drawn | results tarball `7b46bd7fb24e…` |
| 2.2a Spoofing user study | % | n/a | B at most plain VT, and at most A + 2 pp | not measured (pending): the human study has not been run. | n/a |
| 2.2b Simulated-reader spoofing (proxy, veto only) | % | n/a | veto if above plain VT | not measured: there is no screenshot mode. | n/a |
| 2.3 Trust-margin pixels | px | n/a | 0 | not measured: there is no screenshot or pixel-diff mode. | n/a |
| 2.4 Authority leakage | events | n/a | 0 | not measured: no terminal sinks are registered. | n/a |
| 2.5 Decoder robustness | CPU-hours per format | n/a | section 2.5 | not measured: there is no decoder fuzzing harness. | n/a |
| 2.6 Denial of service, flood budget | budget violations | within budget, 500 of 500 blocks drawn | within the 25% unauthenticated share | pass for the byte pipeline over a 30 s flood. The criterion proposes 10 minutes, so this is a shorter run | results tarball `7b46bd7fb24e…` |
| 2.6 Denial of service, peak memory | MiB | 3 MiB in the report | under 512 MiB | pass for the driver process, not a terminal. The run report gives 3 MiB for each run. This is a sampled peak, so it varies between runs (a re-run of the flood measured a higher value, 6 MiB), and every value seen is far under the limit | results tarball `7b46bd7fb24e…` |
| 2.6 Denial of service, longest feed | ms | 6.07 ms | UI stall under 2 s (proposed) | reported only: the longest single call into the layer is a proxy, not the UI stall | results tarball `7b46bd7fb24e…` |
| 2.6 Denial of service, UI stall, frame rate, lying decoder | n/a | n/a | section 2.6 | not measured: the driver has no UI or decoder. | n/a |
| 2.7 Red-team findings | open findings at Medium or above | n/a | 0 | not measured: this is a manual independent review. | n/a |
| 2.8 Cost | reported only | n/a | none | not measured: the cost mode was not merged at this commit. | n/a |
| 2.9 Parity | channels | n/a | 0 | not measured: the parity mode was not merged at this commit. | n/a |
| 7.x Multiplexer placement | various | n/a | section 7.3 | not measured: there is no pane corpus or harness. | n/a |

#### rt-2: the sealed path is exercised

`sealed_opened` totals 261 in this run. Per case:

| Case | Result |
|---|---|
| `sealed-valid` | 1 opened |
| `sealed-valid-two-in-order` | 2 opened |
| `sealed-valid-split-across-reads` | 1 opened |
| `sealed-replayed-frame` | 1 opened, 1 refused as a replay |
| `sealed-over-body-cap-in-one-second` | 256 opened, 44 refused |
| `sealed-tampered-body` | 0 opened |
| `sealed-abort-mid-body` | 0 opened |
| `sealed-wrong-keys` | refused as an invalid header |

The run report gives these per-case results for the sealed arms together. The case `sealed-aborts-pin-the-layer` is not listed there.

#### Decision outcome

No outcome under section 4. Criteria 2.2a, 2.3 and 2.9 are not measured, and 2.1 is only partly measured, so the rule cannot be evaluated. Gates G1, G6 and G7 are not met until the macOS clean-VM proof is posted.

#### Earlier run at 726cb4a: per-case output (rt-1)

Generated from the evidence files by a script, not typed in.

| Case | Run | Items | Violations | sealed_opened |
|---|---|---|---|---|
| `spoof-sealed-type-forged` | arm b | `{"text": 1}` | 0 | 0 |
| `wrong-tag-then-real-frame` | arm b | `{"block": 1, "text": 1}` | 0 | 0 |
| `introducer-inside-rejected-header` | arm b | `{"text": 2}` | 0 | 0 |
| `over-limit-length-no-swallow` | arm b | `{"block": 1, "text": 1}` | 0 | 0 |
| `raw-c1-introducer-outside-utf8` | arm b | `{"block": 1}` | 0 | 0 |
| `utf8-continuation-not-introducer` | arm b | `{"output": 1}` | 0 | 0 |
| `alt-screen-no-rich` | arm b | `{"text": 1}` | 0 | 0 |
| `tmux-no-rich` | arm b | `{"text": 1}` | 0 | 0 |
| `no-container-no-block` | arm b | `{"background": 1}` | 0 | 0 |
| `control-in-text-escaped` | arm b | `{"text": 1}` | 0 | 0 |
| `truncated-frame-at-end` | arm b | `{"text": 1}` | 0 | 0 |
| `split-introducer-across-reads` | arm b | `{"block": 1}` | 0 | 0 |
| `orphan-flood-budget-spent` | arm b | `{"background": 500, "degraded": 1, "text": 100}` | 0 | 0 |
| `orphan-under-multiplexer` | arm b | `{"text": 1}` | 0 | 0 |
| `track0-payload-introducer-trigger` | arm b | `{"text": 2}` | 0 | 0 |
