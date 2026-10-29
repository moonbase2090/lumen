# Results: Arm B (Track 0 everywhere plus the sealed upgrade)

Second comparison run, at main `7529aca649448252111ca79f3c55a38f7d11aadc`. The first run (at 726cb4a) is kept at the end of this file.

This run measures parts of 2.1, 2.2 and 2.6 and nothing else. Section 4 of the [criteria](comparison-criteria.md) cannot be applied: 2.2a, 2.3 and 2.9 are not measured.

## What this run does and does not show

- **2.2 checks byte-pipeline output only.** The driver replays corpora through the arm's byte pipeline and counts forbidden items. Nothing is drawn, so it does not show that a terminal would draw no forged boundary, exit status, producer identity or badge.
- **Reach uses the item-kinds oracle, not screenshots.** Criterion 2.1 names the screenshot-diff oracle, so the reach figure below is a partial measurement and is not counted as the 2.1 result.
- **The macOS reach rows are modelled on Linux.** The harness models the macOS sessions on the Linux VM. Nothing ran on macOS.
- **The test pass rests on exit status.** The workspace tests passed because the run exited 0 under `pipefail`. The per-test lines were cut off by the console capture, so they are not part of this result. The capture is to be fixed for the next run.
- **The G1, G6 and G7 gates are not met** until the macOS clean-VM proof for the gate experiments is posted. This run does not switch any gate on or off.

## Run

- Build commit: `7529aca649448252111ca79f3c55a38f7d11aadc` (main, which includes pull requests 35 to 39)
- Machine: clean Linux VM, arm64, Ubuntu 24.04, unprivileged user, built with `--locked`
- Harness: `lumen-evidence-2`. Session matrix: `sm-1`. Corpora: `rt-1` (15 cases) and `rt-2` (9 sealed-frame cases).
- Overall: `cargo test --locked --workspace` and the release build passed, and all 11 evidence runs exited 0.
- Raw logs: the results tarball with sha256 `7b46bd7fb24ed5f1deaf78b587fc3ccb1ba0487b5639a956323b17224debbe7b` holds the 11 `run.jsonl` and `summary.txt` pairs. It is referenced by hash only and is not committed.

## Metrics

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

## rt-2: the sealed path is exercised

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

## Decision outcome

No outcome under section 4. Criteria 2.2a, 2.3 and 2.9 are not measured, and 2.1 is only partly measured, so the rule cannot be evaluated. Gates G1, G6 and G7 are not met until the macOS clean-VM proof is posted.

## Earlier run at 726cb4a: per-case output (rt-1)

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
