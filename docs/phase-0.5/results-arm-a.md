# Results: Arm A (sealed-only rich)

This is the first comparison run, at one commit, over one corpus. It measures one criterion in part (2.2, byte pipeline only) and no other criterion. Section 4 of the [criteria](comparison-criteria.md) cannot be applied yet.

## Run

- Build commit: `726cb4a92fc3604e10f35e4192c322f4864c1f12`
- Machine: clean Linux VM, arm64, Ubuntu 24.04.5, rustc 1.99.1 (58cf8f9c3 2026-10-13), built with `--locked`
- Corpus: `docs/phase-0.5/corpus/`, revision `rt-1`, 15 cases, sha256 `4d840cfbddc6f38ad4b08e60e46e78d5215a9f94fb45dac5b877c5ba8bf40df0`
- Harness and session matrix: the driver records neither a harness version nor a session-matrix revision. No session matrix exists yet.
- Tests run first on the same VM (`cargo test --locked -p lumen-evidence -p lumen-harness -p lumen-arm-a -p lumen-arm-b`): all passed, 0 failed. Output: [evidence/726cb4a-tests.txt](evidence/726cb4a-tests.txt)
- Gates that were off: none were switched on or off in this run.

## Metrics

| Metric | Unit | Value | Threshold | Result | Raw log |
|---|---|---|---|---|---|
| 2.1 Reach | % | n/a | B at least 80% (12/14) | not measured: there is no session matrix and no session-replay mode, and the corpus has no `expect` field yet (section 5.1). | n/a |
| 2.2 Spoof success | forbidden items over rt-1 | sealed layer: 0; unsealed layer: 0 | 0 | 0 in every run. See the note below: byte pipeline only, nothing is drawn | [726cb4a-rt-1-arm-a-sealed.txt](evidence/726cb4a-rt-1-arm-a-sealed.txt), [726cb4a-rt-1-arm-a-unsealed.txt](evidence/726cb4a-rt-1-arm-a-unsealed.txt) |
| 2.2a Spoofing user study | % | n/a | B at most plain VT, and at most A + 2 pp | not measured (pending): the human study has not been run. | n/a |
| 2.2b Simulated-reader spoofing (proxy, veto only) | % | n/a | veto if above plain VT | not measured: the driver renders nothing and has no screenshot mode. | n/a |
| 2.3 Trust-margin pixels | px | n/a | 0 | not measured: there is no renderer or screenshot, and the driver has no pixel-diff mode. | n/a |
| 2.4 Authority leakage | events | n/a | 0 | not measured: the driver reports `2.4=not measured (no terminal sinks are registered)`; all six sinks show `exercised=0`. | n/a |
| 2.5 Decoder robustness | CPU-hours per format | n/a | section 2.5 | not measured: there is no decoder fuzzing harness, and images stay off. | n/a |
| 2.6 Denial of service | budgets, memory, stall | n/a | section 2.6 | not measured: there is no flood mode. Case `orphan-flood-budget-spent` exercises the budget path only; its counters are in the per-case table. | n/a |
| 2.7 Red-team findings | open findings at Medium or above | n/a | 0 | not measured: this is a manual independent review. | n/a |
| 2.8 Cost | reported only | n/a | none | not measured: there is no cost mode in the driver. | n/a |
| 2.9 Parity | channels | n/a | 0 | not measured: there is no parity mode in the driver. | n/a |

### What check 2.2 covers

The evidence driver replays the corpus through the arm's byte pipeline and counts items whose kind is in a case's `forbid` list. `violations=0` means no forbidden item came out of the pipeline. Nothing is drawn, so this does not show that a terminal would not draw a forged boundary, exit status, producer identity or badge. The spoof-success percentage is not printed by the driver and is not measured.

Arm A ran in two driver modes, `--arm a-sealed` and `--arm a-unsealed`. On the unsealed layer, Track 0 output is escaped text, so cases with a Track 0 block in Arm B show text here.

## Decision outcome

No outcome under section 4. Criteria 2.1, 2.2a, 2.3 and 2.9 are not measured, so the rule cannot be evaluated.

## Not covered by this corpus

Every case reports `sealed_opened=0`, because rt-1 has no real sealed frames. The sealed upgrade path is therefore not exercised by this run.

## Per-case output

Generated from the evidence files by a script, not typed in.

| Case | Run | Items | Violations | sealed_opened |
|---|---|---|---|---|
| `spoof-sealed-type-forged` | sealed layer | `{"text": 1}` | 0 | 0 |
| `wrong-tag-then-real-frame` | sealed layer | `{"block": 1, "text": 1}` | 0 | 0 |
| `introducer-inside-rejected-header` | sealed layer | `{"text": 2}` | 0 | 0 |
| `over-limit-length-no-swallow` | sealed layer | `{"block": 1, "text": 1}` | 0 | 0 |
| `raw-c1-introducer-outside-utf8` | sealed layer | `{"block": 1}` | 0 | 0 |
| `utf8-continuation-not-introducer` | sealed layer | `{"output": 1}` | 0 | 0 |
| `alt-screen-no-rich` | sealed layer | `{"text": 1}` | 0 | 0 |
| `tmux-no-rich` | sealed layer | `{"text": 1}` | 0 | 0 |
| `no-container-no-block` | sealed layer | `{"text": 1}` | 0 | 0 |
| `control-in-text-escaped` | sealed layer | `{"text": 1}` | 0 | 0 |
| `truncated-frame-at-end` | sealed layer | `{"text": 1}` | 0 | 0 |
| `split-introducer-across-reads` | sealed layer | `{"block": 1}` | 0 | 0 |
| `orphan-flood-budget-spent` | sealed layer | `{"degraded": 1, "text": 600}` | 0 | 0 |
| `orphan-under-multiplexer` | sealed layer | `{"text": 1}` | 0 | 0 |
| `track0-payload-introducer-trigger` | sealed layer | `{"text": 2}` | 0 | 0 |
| `spoof-sealed-type-forged` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `wrong-tag-then-real-frame` | unsealed layer | `{"text": 2}` | 0 | 0 |
| `introducer-inside-rejected-header` | unsealed layer | `{"text": 2}` | 0 | 0 |
| `over-limit-length-no-swallow` | unsealed layer | `{"text": 2}` | 0 | 0 |
| `raw-c1-introducer-outside-utf8` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `utf8-continuation-not-introducer` | unsealed layer | `{"output": 1}` | 0 | 0 |
| `alt-screen-no-rich` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `tmux-no-rich` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `no-container-no-block` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `control-in-text-escaped` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `truncated-frame-at-end` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `split-introducer-across-reads` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `orphan-flood-budget-spent` | unsealed layer | `{"degraded": 1, "text": 600}` | 0 | 0 |
| `orphan-under-multiplexer` | unsealed layer | `{"text": 1}` | 0 | 0 |
| `track0-payload-introducer-trigger` | unsealed layer | `{"text": 2}` | 0 | 0 |
