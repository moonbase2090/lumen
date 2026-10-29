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

Lumen is in **Phase 0.5**. On the owner's decision, v0 uses Arm B: Track 0 rich everywhere, with the sealed upgrade, and multiplexer panes keep the sealed relay. The evidence, and what is not yet measured, is in [the decision record](https://github.com/moonbase2090/lumen/blob/main/docs/phase-0.5/decision.md).


Phase 1 has started on Arm B. The terminal runs programs on the upstream VT core and draws the trust margin itself. A sandboxed image decoder, a grid parser that drops string escapes, a notification path (off by default, with no operating-system backend yet), and the first Linux sealing probe are in the repository. Nothing here is released, and no gate result is claimed for any of it. The progress is listed in the [README](https://github.com/moonbase2090/lumen#status).

</section>
