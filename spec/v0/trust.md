+++
title = "Unverified output and the trust margin"
weight = 90
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Unverified output (Track 0) and the trust margin {#trust}

Inert rich blocks (Track 0) render by default on every supported platform, including macOS and over SSH, from any program's output, subject to the [multiplexer](@/spec/v0/multiplexers.md) and [alternate-screen](@/spec/v0/multiplexers.md#mux.alt-screen) rules. They can never create command structure, touch the trust margin or exercise a power. The four MUST conditions below are the core of v0.

## Scope {#trust.track0-scope}

Track 0 includes paragraph, proportional runs, heading, code, table, section and collapsible, progress and status (content only), links (confirmation required to open) and images (subject to [Images](@/spec/v0/images.md)).

Track 0 does **not** include cell spans inside grids; command, prompt and exit types; clipboard; notifications; grants; title; or Track B. These are dropped. An attempt to use them renders as inert text or as a chrome count of dropped items.

## MUST 1: no command structure, no trust margin {#trust.must1-structure}

- Unsigned output MUST NOT create, close or alter command structure. That includes command, prompt or output boundaries, exit status, producer identity, verified badges, and outline or jump targets.
- Unsigned output MUST sit in a terminal-drawn container labelled **unverified output**. It can open blocks only inside that container, cannot address the ids of other producers, and is clipped to the container: no negative margins, absolute positioning or overlay.
- Unsigned output MUST NOT paint any pixel of the **trust margin**, which is the gutter and header strip where the terminal draws producer labels, badges, exit status and boundaries.
- A depth and length cap stops a producer from pushing the next real prompt off-screen without a collapse marker drawn by the chrome. Outline entries from Track 0 carry the unverified label.

## Container lifecycle {#trust.container-lifecycle}

- Only the terminal opens and closes the unverified output container. When it closes the container, it force-closes any open Track 0 blocks.
- On a sealed layer, the container closes only on the sealed command-end frame. Foreground job exit and process-group changes never close it.
- On an unsealed layer, the terminal closes the container when the foreground job exits.
- Track 0 never extends or reopens a closed container.

## Background output {#trust.background}

Track 0 can arrive while no terminal-opened container is open: after a close, at or before the prompt, or from background or daemonised processes. Such output goes into one collapsed **background output (N)** strip drawn by the terminal. There is one strip per layer (per pane under a Lumen-aware multiplexer) per prompt cycle, placed below the input editor. It opens only on user action and never shifts the prompt or editor. It uses that producer's single unsigned budget. When that budget is spent, or when policy sets `background_track0 = text`, further orphan output renders as escaped text.

## MUST 2: decoder gate {#trust.must2-decoder}

Images in unsigned output are decoded only by the out-of-process sandboxed decoder, and only on a platform where the [decoder gate](@/spec/v0/conformance.md#gate.decoder) and the [decoder memory gate](@/spec/v0/conformance.md#gate.decoder-memory) have passed. Otherwise, and after a decoder crash for that producer and command, the alt text is shown. See [Images](@/spec/v0/images.md) for the full rule.

## MUST 3: links {#trust.must3-links}

- A link never opens automatically.
- Activation always needs an out-of-content confirmation drawn by the terminal. It shows the full decoded target (punycode shown, host never truncated), the producer (unverified output) and a mismatch flag when link text that looks like a URL differs from the target. This applies even where policy would skip the confirmation for sealed layers.
- Only `https`, `http` and `mailto` are allowed. `file:` and every other scheme are refused.
- Further rules are in [Links](@/spec/v0/links-and-clipboard.md#links).

## MUST 4: budgets {#trust.must4-budgets}

Each unauthenticated producer gets a smaller share than any sealed layer: **25% of each budget**. This covers frame size, blocks, depth, table cells, shaping cache, patch rate, frame and byte rates, and decoded image bytes. It never touches the reserved local share. When over budget, that producer degrades to **plain grid text** for the rest of the command, with a chrome notice, rather than entering a throttle loop. The hidden-run and visibility rules in [Text runs](@/spec/v0/text-runs.md#runs.visible) apply unchanged.

## Trust leaks {#trust.leaks}

"Verified" means that this command ran and exited with this status. It never means that the command's content is trustworthy, and the UI says so. Output of a sealed command that arrives as Track 0 is still unverified output. Copying across producers shows the split in its confirmation. Link open and paste-as-command never take text from an unverified container without the visible confirmation.

## Notifications {#trust.notifications}

The terminal intercepts OSC 9, OSC 777 and OSC 99 in a layer before any emulator sees them. This holds on every path: grid output, and Track 0 output with or without an open container. Each one is routed to the terminal's notification handling and is never passed to the grid as a sequence.

A notification is either a **terminal toast**, drawn in the terminal's own chrome, or an **OS notification**, raised through the operating system. Unsigned output never raises an OS notification.

- **Default for unsigned output: deny.** Notifications from unsigned output draw no toast and no OS notification. The attempt is counted in the chrome count of dropped items ([Track 0](@/spec/v0/trust.md#trust.track0-scope)).
- **Opt-in for unsigned output.** The policy key `notifications.allow-unsigned` (default `false`, set only in the user's policy file, [Policy](@/spec/v0/capabilities.md#caps.policy)) lets unsigned grid output on a local layer draw a toast, at most one per layer per second. Unsigned grid output is a byte stream with no frame boundary, so the unit is time: any notification after the first in the same second on that layer is dropped and counted in the chrome count. It never applies to possibly-remote layers, SSH sessions or multiplexer panes. It never produces an OS notification. This is an explicit exception to [No authority from unsealed input](@/spec/v0/conformance.md#gate.no-authority), which covers all other unsealed input unchanged.
- **A sealed 0x22 frame with the `notifications` capability** draws at most one toast or OS notification per frame, and any further notification from that frame is dropped and counted in the chrome count. The capability is per sealed layer, as [Grants](@/spec/v0/capabilities.md#caps.grants) describes, and it is not in the default grant for a remote layer. The frame type is in the frame-type table of the transport spec.

**OS notifications** are off by default. The policy key `notifications.os` (default `false`, set only in the user's policy file) allows them. When it is on, a sealed 0x22 notification with the `notifications` capability raises an OS notification only if all of these hold:

- the layer is not possibly-remote, not an SSH session and not a multiplexer pane. No OS notification can come from those layers in v0;
- the terminal is unfocused. A focused terminal shows a toast instead;
- the pane is not muted. Each pane can be muted by the user;
- the layer is within its rate limit, at most one OS notification per layer per second.

The terminal composes the OS notification text itself from a fixed terminal name, the pane label and the program's plain text only. The pane label is the layer's position in the terminal's own layout (window, tab or pane index) and its colour, as [Pane identity](@/spec/v0/multiplexers.md#mux.pane-identity) describes for panes. It is never a program-supplied title, working directory or pane text. The text is length-capped and escaped. No program icon, URL, action button or sound is passed to the operating system. Clicking an OS notification only focuses the pane. It never executes anything.

**Accepted residual risk.** A signed program can write phishing-style wording into the plain text of its notification. The pane label and the `notifications` capability are the mitigations. The wording itself is not checked, and this risk is accepted.

**Red-team pass.** This section does not count toward any gate until a red-team pass by a reviewer other than its author has been recorded.

## Unverified boundaries {#trust.unverified-boundaries}

Without a sealed layer, the terminal MAY infer command boundaries from its own process tracking, such as foreground process-group changes and alternate-screen exit. It draws them in the trust margin itself, clearly labelled **unverified boundary**, and never claims an exit status for them. Output bytes cannot create or move them, although a program can cause one through job control. A sealed layer replaces them with verified boundaries and exit status.

## Possibly remote {#trust.possibly-remote}

Unsealed Track 0 is treated as **possibly remote** on every layer. The `ssh.track0` policy sets how it renders. It governs all unsealed Track 0 and is named for its original SSH case. Its default is `rich`: Track 0 blocks render as inert content under the rules of this section. Its other value is `text`, under which all possibly-remote Track 0 renders as escaped text. The policy is read only from the user's policy file, as [Policy](@/spec/v0/capabilities.md#caps.policy) describes. A Lumen-aware multiplexer reached over SSH stays text only in v0, whatever this policy says ([remote multiplexers](@/spec/v0/multiplexers.md#mux.remote)).

An **SSH session** is a layer the terminal created for an SSH profile, or any layer whose foreground process tree contains an `ssh` client. When the terminal cannot tell whether a layer is an SSH session, it treats the layer as one. Under either value of `ssh.track0`, an image in Track 0 on an SSH session is not decoded. It shows its alt text as escaped text, whatever the state of the [decoder gate](@/spec/v0/conformance.md#gate.decoder), and it counts against the unsigned budget.

Only an explicit local policy set by the user in terminal settings lifts possibly-remote status, per profile: `track0.treat_unsealed_as_local = true`. Setting it shows a persistent warning in profile settings and a chrome notice on layers that use it. The lift changes how unsealed Track 0 renders under `ssh.track0`, but it does not change the image rule: an image in Track 0 on an SSH session is not decoded whatever that policy says. The lift never applies to multiplexer panes or to SSH layers created by the terminal. No signal in the output, environment or process tree ever lifts possibly-remote status. Sealed frames are classified by their layer.
