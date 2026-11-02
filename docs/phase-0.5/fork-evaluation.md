# Phase 0.5 fork evaluation: Arm A against Arm B

The figures here come from the third run recorded in the results files. The decision record cites a later rerun at `d8994bb` with different latency figures (6.7 ms for Arm B, 8.8 to 9.4 ms for Arm A); that rerun is not in these files. This page records, for each open fork, the result against the criteria in [the criteria](comparison-criteria.md) and the decision rule in [section 4](comparison-criteria.md#4-decision-rule). It adds no measurement and changes no criterion. Every figure is copied from [Arm A](results-arm-a.md), [Arm B](results-arm-b.md) and [placement](results-placement.md) (third run, main `e276673`, clean Linux VM), and the choice itself is in [the decision record](decision.md).

## Fork 1: Arm A (sealed-only rich) against Arm B (decoupled)

| Criterion | Arm A | Arm B | Threshold | Status |
|---|---|---|---|---|
| 2.1 Reach | 14/14 on the sealed layer; unsealed n/a | 14/14 | B at least 12/14 | Partial. Item-kind oracle, not the screenshot oracle; macOS rows modelled on Linux. |
| 2.2 Spoof success (rt-1 to rt-4, 95 cases) | 0 | 0 | 0 | Met for the byte pipeline. Nothing is drawn. |
| 2.2a Human spoofing study | n/a | n/a | B no worse than plain VT, at most A + 2 pp | Not run. Open. |
| 2.2b Simulated-reader veto | n/a | n/a | veto if above plain VT | Proposal only. No run recorded. |
| 2.3 Trust-margin pixels | n/a | n/a | 0 | No run recorded. Open. |
| 2.4 Authority leakage (rt-4) | 0 on the sealed layer; unsealed not measured | 0 | 0 | Met at the test-model level; confirmations are simulated. |
| 2.5 Decoder robustness | n/a | n/a | release criteria | Not measured; shared by both arms. |
| 2.6 Flood (600 s) | within budget; longest call 6.4 to 6.7 ms | within budget; longest call 6.4 ms | 25% share, 512 MiB | Met for the byte pipeline. Proxy only. UI stall and lying decoder not measured. |
| 2.7 Red-team review | n/a | n/a | 0 Medium or higher | Not reviewed. |
| 2.8 Cost | +1481 lines | +1340 lines | reported only | Reported. No new dependency or `unsafe` in either arm. |
| 2.9 Parity | n/a | n/a | 0 | Declaration level only. |

### Reading against the rule

- **Step 1, preconditions.** Gates G1, G6 and G7 are not met until the macOS clean-VM proof is posted, so neither arm's rich rendering is switched on by this evidence.
- **Step 2.** No measured clause fails Arm B. Two clauses that could fail it are unmeasured (2.3) or pending the human study (2.2a). The 2.2b veto is not part of the confirmed rule.
- **Step 3.** Arm B meets 12/14 on the item-kind oracle only.
- **Step 5.** Parity, 2.2, 2.6 and 2.7 are at declaration, byte-pipeline, proxy and unreviewed level; each must be fixed or waived in writing before acceptance.

**Outcome.** The rule has no outcome yet: 2.2a, 2.3 and 2.9 are not measured and 2.1 is partial. The maintainers chose Arm B anyway (Track 0 on every layer, as the spec's Track 0 rules describe), and Arm A is frozen as the evidence record. The measured clauses give no reason to reverse that choice: they are at parity on safety and reach, and Arm B is smaller by 141 lines. The latency proxies are equal within noise in this run. They do not show Arm B passes the open clauses, and no waiver is recorded.

## Fork 2: multiplexer placement (Option A sealed relay against Option B unsealed producer)

At the model level, Option A had 0 violations over 18 cases, and in the six hostile-multiplexer cases dropped 3 of 3 stale-generation containers, refused 2 of 2 replays and opened 0 of 4 pane-written sealed-looking frames. Option B had 0 violations, but draws no per-pane containers, so its zeros have nothing to count. Key reuse, the replayed mark, trust-margin pixels in panes, key custody and pane reach are not measured, so no platform has a 7.3 outcome. The maintainers keep placement A for multiplexer panes; that is a judgement, not a rule outcome.

## What would close the open clauses

1. Screenshot oracle: reach (2.1) and trust-margin pixels (2.3) on a terminal that draws.
2. The 2.2a protocol and study, and maintainer approval or rejection of the 2.2b veto.
3. Decoder fuzzing (2.5) against the release criteria, and the 2.6 UI stall and lying-decoder runs.
4. The 2.7 red-team review of the judged commit.
5. The macOS clean-VM proof for G1, G6 and G7.
6. A fresh comparison rerun that records the pixel, reader and fuzz results, if those modes have run since main `e276673`; none is recorded in this directory.
