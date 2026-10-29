# Results: multiplexer placement (section 7), model level

First placement run, at main `e27667310b15016c2aa891b1f33d0d5a9750c441`, on the same clean Linux VM run as the third arm results ([results-arm-a.md](results-arm-a.md), [results-arm-b.md](results-arm-b.md)). Raw logs: the results tarball with sha256 `7f4c99f934a804c846649dfd87e1db37698dd8c7da95e5a1903ae5ea205c4b7a`, referenced by hash only and not committed. Harness `lumen-evidence-2`, pane corpus `pc-1` (18 cases, revision `pc-1`).

This compares the two placements at the model level, as [7.6](comparison-criteria.md) describes: Option A is the sealed relay and the outer terminal's pane handling in `lumen-panes`, and Option B feeds the panes' bytes to an ordinary unsealed producer's layer. Nothing is drawn, nothing runs on a real multiplexer, and the model is one process.

## Counts

| Metric (7.2) | Option A | Option B | Threshold |
|---|---|---|---|
| Draws outside a pane rectangle | 0 | 0 | 0 |
| Draws from a hidden or detached pane | 0 | 0 | 0 |
| Stale-generation containers drawn | 0 | 0 | 0 |
| Forged containers drawn | 0 | 0 | 0 |
| Replayed frames accepted | 0 | 0 | 0 |
| Stale-generation drop | 3 of 3 dropped | 0 of 0 (no per-pane containers) | all dropped |
| Replayed frames refused | 2 of 2 | 0 of 0 | all refused |
| Pane-written sealed-looking frames that opened when passed to the outer pipeline | 0 of 4 | 0 of 0 | 0 |
| Violations over the 18 cases | 0 | 0 | 0 |

## What a zero means

- **Twelve of the 18 cases drive an honest multiplexer.** Every count in them is zero by construction. They show that the rules are consistent and would catch a regression in the relay; they say nothing about a multiplexer that misbehaves.
- **Six cases play a hostile multiplexer** and can fail. For Option A they gave: 3 of 3 out-of-generation containers dropped, 2 of 2 replays refused, and 0 of 4 pane-written sealed-looking frames opened by the outer terminal's pipeline. That is the positive evidence in this run, and it is evidence about the model's rules, not about a real implementation.
- **Option B's zeros are not evidence.** It draws no per-pane containers in this model, so its drop, refuse and forged counters have nothing to count (0 of 0). Its outcome is "no rich panes", which is the Option B outcome of 7.3, not a result of passing a test.

## Not measured

- Key reuse on reattach, and the replayed mark: the model picks fresh keys and the frames carry no replayed flag.
- Trust-margin pixels in panes and pane reach: nothing is drawn, and the screenshot oracle does not exist yet.
- Key custody: the model is one process, so keys in a server's memory, environment or socket cannot be observed. The real socket checks (peer credentials, directory ownership) are the separate OS-touching work.

## Placement decision

No platform has a placement outcome under 7.3. Option A is selected on a platform when every gated metric has zero failures there and the multiplexer gate passes there. Four gated metrics (trust-margin pixels, key custody, the replayed mark and key reuse) are not measured, and the multiplexer gate is not measured, so the condition cannot be shown. Option B is selected where Option A's gate does not pass; that is not shown either. This run neither selects Option A nor falls back to Option B on any platform. It narrows the open questions to the unmeasured metrics above.
