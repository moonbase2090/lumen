# Phase 0.5 decision: Arm B

On the owner's decision, the project chose **Arm B** (Track 0 rich everywhere, with the sealed upgrade) as the design for v0. Multiplexer panes keep placement A (the sealed relay). Arm A is frozen as the evidence record: it stays in the repository, and its results are kept as they were measured.

This page records the decision and the evidence behind it. It is not a new measurement, and it does not change the criteria or the decision rule in [the criteria](comparison-criteria.md), which were fixed before the arms were built.

## The evidence

All figures are from the final comparison rerun at main `d8994bb`, on a clean Linux virtual machine ([results](results-arm-b.md), [results for Arm A](results-arm-a.md)).

- **Safety, parity.** Both arms show zero forbidden items across 95 red-team cases (byte-pipeline output only), and both show zero authority leaks on the sealed layer. The leak zero comes from a test model of a terminal, with clipboard and link confirmations simulated, so it is evidence about the model and not about a built terminal.
- **Reach.** Both arms reach 14 of 14 sessions on the item-kind oracle. That oracle is not the screenshot oracle the criteria name.
- **Size.** Against the same base, Arm B adds 1340 lines of code and Arm A adds 1481. Neither adds an external dependency beyond those the shared crates already have, and neither adds an `unsafe` use.
- **Latency.** In one 600 second flood per arm, the longest call into the layer was 6.7 ms for Arm B and 8.8 to 9.4 ms for Arm A. This is a proxy for a UI stall and not a measured stall, and it is a single run per arm.
- **Matching Track 0 and the spec.** Arm B renders Track 0 on every layer, as the spec describes in the Track 0 rules. Arm A renders it only on sealed layers, which the spec states only for alternate-screen and multiplexer cases.

## What is not measured

These criteria have no result, so they play no part in the choice:

- the human spoofing study (2.2a) and the simulated-reader proxy (2.2b);
- trust-margin pixels (2.3), which need a terminal that draws;
- decoder robustness (2.5), which needs a decoder;
- the UI stall, frame rate and lying-decoder memory of 2.6;
- the red-team review (2.7);
- side-channel parity beyond the declaration level (2.9);
- 2.4 for Arm A on an unsealed layer, which has no sealed frame to exercise the sinks;
- the section 7 gates for key reuse, the replayed mark, trust-margin pixels in panes, key custody and pane reach.

The decision therefore rests on the measured criteria above, on the owner's judgement, and on the spec. It does not claim that any unmeasured criterion passed.

## Relation to the decision rule in section 4

The choice of Arm B is the owner's decision. It is not the outcome of the rule in [section 4](comparison-criteria.md#4-decision-rule), and this record does not say that the rule selects Arm B. The rule's clauses stand as follows:

- **Step 2, trust-area pixels (2.3).** Not measured. This clause is open.
- **Step 2, authority leaks (2.4).** Zero on the sealed layer, but at the level of the test model with simulated confirmations. The clause is met for the model, and the model-level limit stands.
- **Step 2, spoofing user study (2.2a).** Not run. The clause is decided by the human study and is open.
- **Step 2, simulated-reader veto (2.2b).** A proposal, not yet part of the confirmed rule. Not measured.
- **Step 3, reach (2.1).** 14 of 14 on the item-kind oracle. The criterion names the screenshot-diff oracle, so the clause is not met on the criterion's own oracle, and it is open until it is measured that way.
- **Step 5, parity (2.9), spoof success (2.2), DoS (2.6) and red-team findings (2.7).** Parity is at declaration level; the 2.2 and 2.6 figures are byte-pipeline and proxy results; 2.7 is not yet reviewed. Each must be fixed or waived in writing before the result is accepted.

**No waivers are recorded in this file.** Each open clause above remains open. If a measured clause fails, the decision is reopened. Any waiver the owner gives goes in this file, in the owner's words, with the clause it covers.

## What follows

- Arm A remains in the tree as the evidence record until the owner decides whether it leaves the default build.
- Work on the unmeasured criteria continues as Phase 1 work, against the design that was chosen.
