+++
title = "Lumen"
template = "index.html"
+++

<section class="section">

## Not backwards compatible, by design

Lumen is a new protocol rather than another escape-code extension. Grid blocks give existing terminal apps a native place inside a Lumen document, and unverified output is always shown as plain, inert text.

</section>
<section class="section">

## Status

The current public draft is **Lumen Protocol v0, revision 25 (frozen snapshot)**. It is not stable, and breaking changes are expected. [Read the spec](@/spec/v0/_index.md).

Lumen is in **Phase 0.5**, a comparison of two candidate designs before v0 is chosen. The first two arms are built in the repository, and the criteria and results are public. The latest rerun, on a clean Linux machine, passed all 276 tests and found no forbidden items over the four red-team corpora for either arm (byte-pipeline output only). Reach was 14 of 14 sessions for both arms on the item-kind oracle, a 600 second flood stayed within budget, and authority leakage read zero on the sealed-frame corpus for Arm B and for Arm A on a sealed layer. That zero comes from a test model of a terminal with two simulated user confirmations, so it does not say a built terminal has no leak. The multiplexer placement comparison ran at model level with no violations under either placement. Trust-margin pixels, the simulated-reader check, decoder robustness and side-channel parity are not measured yet, so no outcome under the decision rule exists. [Read the criteria and results](https://github.com/moonbase2090/lumen/tree/main/docs/phase-0.5).

</section>
