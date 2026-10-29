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


Phase 1 has started on Arm B. The terminal runs programs on the upstream VT core and draws the trust margin itself. A sandboxed image decoder and an image manager that enforces its rules, neither yet connected to the terminal, so images are not shown yet; a grid parser that drops string escapes; a notification path in the window, where sealed notifications draw toasts, a click dismisses a drawn toast, and an operating-system backend that has shown a real desktop banner on Linux in a clean-VM proof, while a notification from a real program in the window is not yet shown; notification settings read only from the user's policy file; and a Linux sealing probe and a key-holder shell that accepts messages only from the terminal's shell and can receive a layer's key, though nothing in the terminal sends one yet. No layer seals yet. Nothing here is released, and no gate result is claimed for any of it. The progress is listed in the [README](https://github.com/moonbase2090/lumen#status).

</section>
