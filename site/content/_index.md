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

Lumen is in **Phase 0.5**, a comparison of two candidate designs before v0 is chosen. The two arms are built in the repository, and the criteria and results are public. The final rerun at main `d8994bb`, on a clean Linux machine, passed 287 tests and ran every comparison mode as expected. Its red-team corpora gave no forbidden items for either arm, which checks byte-pipeline output only, and authority leakage read zero on the sealed layer, from a test model of a terminal with simulated user confirmations. Reach was 14 of 14 sessions for both arms on the item-kind oracle, and a 600 second flood stayed within budget. Trust-margin pixels, the simulated-reader check, decoder robustness, the UI stall and frame rate, and side-channel parity beyond declaration level are not measured yet. [Read the criteria and results](https://github.com/moonbase2090/lumen/tree/main/docs/phase-0.5).

</section>
