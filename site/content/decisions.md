+++
title = "Decisions and accepted risks"
+++

This page records the decisions the maintainers made that shape Lumen Protocol v0, and the residual risks they accepted. It describes each one by its effect and points to the part of the [specification](@/spec/v0/_index.md) that carries the rule. The specification is the authority: if this page and the specification differ, the specification wins and this page is wrong.

Attack details are not published here, in line with the [threat model](@/spec/v0/threat-model.md). To report a problem, use the [private channels](@/security.md).

A decision is listed here only if the specification already states it. Any entry can be challenged or withdrawn by the maintainers.

## Decisions

Every decision listed here is **Adopted**: the specification follows it.

### Shell integration is the only source of command structure

- **Status:** Adopted.
- **Effect:** Command boundaries and exit status come only from authenticated frames sent by the terminal-owned shell integration, or from the terminal's own labelled, unverified boundaries. A program's own output cannot create them.
- **Where:** [Document model](@/spec/v0/document-model.md), [Unverified output](@/spec/v0/trust.md#trust.unverified-boundaries).

### Inert rich output is allowed from any program, powers are not

- **Status:** Adopted.
- **Effect:** Any program can emit the inert block types, which render inside a labelled unverified container. Rendering inert content is not authority. Command structure and every power stay sealed-only.
- **Where:** [Goals](@/spec/v0/_index.md#goals), [Block types](@/spec/v0/blocks.md), [Unverified output](@/spec/v0/trust.md#trust.track0-scope).

### Each layer has its own key-holder process

- **Status:** Adopted.
- **Effect:** Keys live in a separate process per layer and reach only the listed shell's integration, over an inherited descriptor. They never appear in the environment, in arguments or on the PTY.
- **Where:** [Sealed layers](@/spec/v0/sealed-layers.md#sealed.requirements).

### Other programs run by the same user are out of scope by default

- **Status:** Adopted, with an optional hardened mode.
- **Effect:** In default mode the model is parity with existing terminals: a process that already runs as the user and attacks the terminal, the shell or a key holder is not defended against. Hardened mode narrows this and is off by default.
- **Where:** [Threat model](@/spec/v0/threat-model.md), [Optional hardened mode](@/spec/v0/hardened-mode.md).

### Hardened mode needs a root-owned install

- **Status:** Adopted, for hardened mode only.
- **Effect:** In hardened mode the terminal and holder binaries are root-owned so the kernel marks them non-dumpable from exec. A user-space install does not get shell integration in hardened mode.
- **Where:** [Optional hardened mode](@/spec/v0/hardened-mode.md#hardened.enable).

### Colour queries get fixed-format replies

- **Status:** Adopted.
- **Effect:** The terminal answers colour queries only in the single query form, from its own theme values with a fixed format, and rate-limits all replies. The privacy effect of these replies is listed under accepted risks below.
- **Where:** [Terminal replies](@/spec/v0/sealed-layers.md#sealed.replies), [Accepted residual risks](@/spec/v0/threat-model.md#threat.residuals).

### tmux and screen are post-v0; one multiplexer path exists

- **Status:** Adopted.
- **Effect:** In v0, Track 0 on a layer inside tmux or screen renders as escaped text. The only multiplexer integration is a Lumen-aware multiplexer that uses the sealed multiplexer layer.
- **Where:** [Multiplexers](@/spec/v0/multiplexers.md#mux.tmux-screen), [Lumen-aware multiplexer](@/spec/v0/multiplexers.md#mux.lumen-aware).

### Flatpak, Snap and rootless containers are unsupported in v0

- **Status:** Adopted.
- **Effect:** No Flatpak build is provided. A terminal that detects such an environment creates no keys and shows a notice.
- **Where:** [Platform support](@/spec/v0/platforms.md#platforms.containers).

### Images are on, behind the decoder gates

- **Status:** Adopted. Images are on in the build that ships, on Linux, macOS and Windows.
- **Effect:** Images are enabled on a platform only after the decoder gate and the decoder memory gate pass for the decoder build, and for the terminal's image path. A change to either re-runs the gates. The maintainers accepted the earlier guarded fuzz runs (PNG, JPEG and WebP, each past 250 CPU-hours, no open crash or confirmed hang) for the current identities, with no new fuzzing; see the accepted residual risk below. The setting `images = false` always shows the alt text.
- **Where:** [Images and the decoder](@/spec/v0/images.md#images.unlock), [Conformance gates](@/spec/v0/conformance.md#gate.decoder).

### Release builds are scanned for personal identifiers

- **Status:** Adopted.
- **Effect:** A finding blocks the release. The allowlist has one entry, the publisher name, matched exactly.
- **Where:** [Release hygiene](@/spec/v0/security.md#security.release).

### A release is refused without clean decoder fuzz evidence

- **Status:** Adopted.
- **Effect:** The release check refuses unless the personal-identifier scan passes and the decoder fuzz ledger is present, records no crash, carries the decoder and image-path build identities of the tree being released, and passes the fuzzing thresholds. A missing input is a refusal. There is no waiver, and the check releases nothing.
- **Where:** [Release criteria](https://github.com/moonbase2090/lumen/blob/main/docs/release-criteria.md#the-release-check).

### macOS release binaries are signed with the hardened runtime

- **Status:** Adopted.
- **Effect:** The macOS release gates run on terminal and holder binaries that are signed with a Developer ID certificate and the hardened runtime, with no entitlements. A binary that is unsigned, signed ad hoc, or marked debuggable is refused. A missing certificate is a refusal, with no unsigned fallback and no waiver, and the signing job releases nothing.
- **Where:** [Release criteria](https://github.com/moonbase2090/lumen/blob/main/docs/release-criteria.md#signed-macos-binaries).

## Accepted residual risks

The specification lists the residual effects the maintainers accepted for v0, each rated Low or Informational. None gives unverified output any authority. They are described only by their effects, and the full text is in [Accepted residual risks](@/spec/v0/threat-model.md#threat.residuals). The headings below are the same entries.

**Output and display**

- Theme fingerprinting.
- Reply bytes in line editors.
- Reply-format edge cases.
- Fake prompts after an unverified boundary.
- Alternate-screen and reflow placement.
- Multiplexer passthrough.
- Local-treatment opt-in.
- Signal ambiguity.

**Availability of sealing**

- Corrupted frames.
- Slow shell start.
- Sandboxed and user installs.

**Local timing and same-user processes**

- Local timing issues, and other processes running as the same user, in default mode.

**Image path**

- Earlier-identity fuzz evidence accepted for the first release with images: the guarded runs count for the current identities, no new fuzzing, and the compiler and flags are not part of the release identity. A decoder version bump still re-runs the fuzzing.
- System graphics drivers used at run time without linking are outside the build identity, so a driver update does not re-run the decoder gates.
