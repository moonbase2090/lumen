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

Lumen is in **Phase 0.5**, a comparison of two candidate designs before v0 is chosen. The first two arms are built in the repository, and the criteria and results are public. The second comparison run measured reach on a 14-session matrix (14 of 14, on the item-kind oracle), zero forbidden items over the red-team corpora for each arm (byte-pipeline output only), and 261 sealed frames opened on each sealed arm. Several criteria are not yet measured, including the human spoofing study, trust-margin pixels, authority leakage and side-channel parity, so no outcome under the decision rule exists yet. [Read the criteria and results](https://github.com/moonbase2090/lumen/tree/main/docs/phase-0.5).

</section>
