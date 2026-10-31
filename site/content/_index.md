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

`lumen-term` has native window backends for Linux and Windows behind the off-by-default `window` feature. A dedicated standard Windows test profile built the binary and captured a real native window.

The current public draft is **Lumen Protocol v0, revision 25 (frozen snapshot)**. It is not stable, and breaking changes are expected. [Read the spec](@/spec/v0/_index.md).

Lumen is in Phase 0.5. The Lumen maintainers adopted Arm B for v0: Track 0 rich everywhere, with the sealed upgrade, and multiplexer panes keep the sealed relay. The evidence, and what is not yet measured, is in [the decision record](https://github.com/moonbase2090/lumen/blob/main/docs/phase-0.5/decision.md).


Phase 1 has started on Arm B. The terminal runs programs on the upstream VT core and draws the trust margin itself. A sandboxed image decoder and an image manager enforce the image rules. Neither is connected to the terminal yet, so images are not shown. [Lumen Term's release criteria](https://github.com/moonbase2090/lumen/blob/main/docs/release-criteria.md) set the decoder fuzzing gate. Each image format must reach at least 250 CPU-hours on each platform for both the decoder build identity and the terminal image-path build identity. A run stops only after its last 168 CPU-hours produce no new crash and no new coverage edge. The run is capped at 1200 CPU-hours, and the format has not passed if the stop rule does not hold by then. A new crash ends the run. Fuzzing continues after release, and every decoder version bump requires another run. Status: not met. The JPEG decoder is pinned to zune-jpeg 0.5.16-rc3, a release candidate: 0.5.15, the last stable release, panics on 16 malformed JPEGs found by fuzzing, and rc3 answers all of them without a panic. Those inputs are now regression tests. Because this changes the decoder build identity, any earlier fuzzing pass for JPEG no longer counts and must be run again. A grid parser drops string escapes. The notification path in the window draws sealed notifications as toasts, and a click dismisses a drawn toast. An operating-system backend is implemented and tested on Linux, but the window does not yet show a notification from a real program. Notification settings read only from the user's policy file. A Linux sealing probe and a key-holder shell accept messages only from the terminal's shell. The key-holder shell can receive a layer's key, but nothing in the terminal sends one yet. No layer seals yet. Paragraph and heading text keeps zero-width characters (ZWJ, ZWNJ and variation selectors), ZWSP and WJ act only as line-break controls, and bidirectional controls are closed at every line and block end. These rules are now on by default; a build without default features keeps the earlier rules. Where the terminal composes text a person relies on to judge trust (notifications, link labels and targets, the code-block language label), invisible and bidi code points show as visible escapes such as ⟨U+200B⟩. Nothing here is released, and no gate result is claimed for any of it. The progress is listed in the [README](https://github.com/moonbase2090/lumen#status).

</section>
