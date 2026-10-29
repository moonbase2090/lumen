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

**Interception.** The terminal intercepts OSC 9, OSC 777 and OSC 99 in a layer before any emulator sees them, on every path: grid output, and Track 0 output with or without an open container. The interception covers the 7-bit `ESC ]` and 8-bit C1 introducers, both terminators (BEL and `ESC \`, or the 8-bit ST), a sequence split across reads, and the multi-part form of OSC 99. A sequence whose body exceeds 4 KiB is discarded when it overflows: the rest of the sequence is discarded too, up to its terminator or up to an ESC, CAN or SUB if one comes first, so an unterminated sequence cannot swallow later output. None of it reaches the grid. An intercepted sequence is routed to the terminal's notification handling and is never passed to the grid. The terminal writes no reply to the PTY for any of them, including OSC 99's query and close-report forms, whose answers would be a side channel.

**Local layers.** A layer counts as local only when its foreground process tree is shown to be local. If the terminal cannot tell, the layer is treated as not local. Possibly-remote layers, SSH sessions and multiplexer panes are never local for these rules.

**Two kinds of notification.** A notification is either a **terminal toast**, drawn in the terminal's notification area, or an **OS notification**, passed to the operating system's notification service. Unsigned output never produces an OS notification.

- **Default for unsigned output: deny.** Notifications from unsigned output draw no toast and no OS notification. The attempt is counted in the chrome count of dropped items ([Track 0](@/spec/v0/trust.md#trust.track0-scope)).
- **Opt-in for unsigned output.** The policy key `notifications.allow-unsigned` (default `false`, set only in the user's policy file, [Policy](@/spec/v0/capabilities.md#caps.policy)) lets unsigned grid output on a local layer (see Local layers above) draw a toast, subject to the rate limits below. It never applies to possibly-remote layers, SSH sessions or multiplexer panes, and it never produces an OS notification. This is an explicit exception to [No authority from unsealed input](@/spec/v0/conformance.md#gate.no-authority), which covers all other unsealed input unchanged.
- **A sealed 0x22 frame with the `notifications` capability** draws one toast, or one OS notification when the OS path below applies, per frame. Any further notification from that frame is dropped and counted in the chrome count. The capability is per sealed layer, as [Grants](@/spec/v0/capabilities.md#caps.grants) describes, and it is not in the default grant for a remote layer. The frame type is in the frame-type table of the transport spec.

**OS path.** OS notifications are off by default. The policy key `notifications.os` (default `false`, set only in the user's policy file) allows them. When it is on, a sealed 0x22 notification with the `notifications` capability raises an OS notification only if all of these hold, evaluated when the notification arrives:

- the layer is not possibly-remote, not an SSH session and not a multiplexer pane. A notification from any of those layers is never an OS notification in v0. The layer must also be local (see Local layers above);
- the terminal is unfocused. A focused terminal shows a toast instead;
- the pane is not muted, and the layer is not muted;
- the layer is within the OS budget: at most one OS notification per layer per second, at most 3 per layer in any 60-second window, and at most 8 in any 60-second window across all layers, measured as sliding windows on a monotonic clock.

An OS notification cannot be recalled once sent: it may stay in a history, sync to other devices, or survive the terminal. The budget above is therefore the only limit on what the OS keeps. When the OS budget is spent, the layer's notifications are toasts only until the user acts on that layer (unmutes it or opens it) or ten minutes pass with no notification from it, whichever comes first. The global budget of 8 in any 60-second window is shared by all layers. A notification that fails it is a toast, so one noisy layer can use up the shared OS budget for a minute but cannot take toasts from another layer. A notification that fails the OS conditions is shown as a toast if the toast limits allow, and otherwise dropped and counted.

**Exposure of program text.** By default an OS notification carries only the fixed prefix and the pane label. The program's text is never sent to the operating system unless the separate policy key `notifications.os-text` is set (default `false`, set only in the user's policy file). Enabling it sends the program's plain text to the operating system's notification service. That service may show it on a lock screen, keep it in a history, sync it to other devices, and expose it to other apps with notification access. Sealed output can contain secrets. The setting's description must state this, next to the setting itself.

**Text.** The terminal composes the text of every toast from three parts, and of every OS notification from the first two, plus the third only when `notifications.os-text` is set:

1. a fixed prefix, `Program in <pane label> says:`, which the program cannot remove;
2. the pane label, as defined below;
3. the program's plain text, after these rules. The text is UTF-8, and any character whose Unicode general category is Cc, Cf, Cs, Co, Cn, Zl or Zp is removed. That includes the bidirectional controls, the zero-width characters, the tag characters (U+E0000 to U+E007F), the invisible operators, the line and paragraph separators (U+2028 and U+2029), and the Hangul and other fillers (U+115F, U+1160, U+3164 and U+FFA0), which are category Lo and so are listed by code point. Combining marks (Mn, Mc, Me) are kept only up to one per base character; further marks are removed, and a mark with no base character before it is removed. The text is one line, with no markup and no link made clickable; markup is escaped for the notification API in use. It is capped at 200 bytes and at 60 terminal cells, measured with the terminal's own cell-width table, not by character count. A truncated text ends with a visible marker.

**Pane label.** The pane label is the layer's position in the terminal's own layout (window, tab or pane index), with the colour and glyph derived from its opaque id, as [Pane identity](@/spec/v0/multiplexers.md#mux.pane-identity) describes for panes. It is never program text: no title, working directory or pane text. Layer ids are never reused, as pane ids are. When a layer closes, its queued and shown toasts are removed, so a position is never shown with a toast from a layer that has gone.

**Rate limits.** The numbers below are provisional proposals, to be tuned by measurement. Measured on a monotonic clock, and applied to toasts from `allow-unsigned` and to every sealed notification alike:

- at least 1000 ms must pass between two notifications accepted from the same layer;
- each layer has a queue of at most 8 toasts, and at most 4 toasts from a layer are shown in any 10 seconds;
- at most 16 toasts are shown at once across all layers. Each toast stays for at most 6 seconds, and the terminal removes it sooner if the user dismisses it. When a layer is at its own limit of 4 toasts in 10 seconds, the new notification is dropped and counted, as the rule below states. When the 16-toast cap is full, a new toast from a layer replaces that layer's own oldest shown toast if it has one; otherwise the new notification is dropped. A new toast never removes another layer's toast.

A notification over these limits is dropped and counted in the chrome count.

**Toast placement.** Toasts are drawn only in the terminal's notification area. They never occlude, and are never drawn in, the trust margin, the unverified-boundary labels or the command structure. Every toast is clipped to its own box: no glyph overflows into neighbouring chrome. An unsigned toast carries the unverified-output label as well as the pane label.

**Click and platform content.** The OS notification passes no program icon, no action button, no reply field and no inline image to the operating system. The only interaction is one default activation. The OS callback carries only a token the terminal made for that notification. The token is random, single-use and bound to one layer; it is invalid after that layer closes or the terminal restarts, and a stale token focuses nothing. Activating a notification focuses the pane. It confirms nothing, dismisses nothing and executes nothing. No dock or taskbar badge, no urgency request and no sound is made.

**User control.** Muting a pane silences its toasts as well as its OS notifications. The user can mute the terminal's notifications in one step, and can revoke the `notifications` capability for a layer in one step. Both are reachable from the keyboard.

**Accepted residual risk.** A signed program is authenticated, not trusted: a hostile dependency inside a sealed command qualifies. Such a program can write misleading wording into its notification text, and once an OS notification is sent it cannot be recalled. The fixed prefix, the pane label, the OS budget and the `notifications` capability are the mitigations. The wording itself is not checked, and this risk is accepted by the owner.

**Red-team pass.** This section does not count toward any gate until a red-team pass by a reviewer other than its author is recorded, and the findings of both passes are answered in the text. The second pass is `docs/phase-0.5/red-team-notifications.md`.

## Unverified boundaries {#trust.unverified-boundaries}

Without a sealed layer, the terminal MAY infer command boundaries from its own process tracking, such as foreground process-group changes and alternate-screen exit. It draws them in the trust margin itself, clearly labelled **unverified boundary**, and never claims an exit status for them. Output bytes cannot create or move them, although a program can cause one through job control. A sealed layer replaces them with verified boundaries and exit status.

## Possibly remote {#trust.possibly-remote}

Unsealed Track 0 is treated as **possibly remote** on every layer. The `ssh.track0` policy sets how it renders. It governs all unsealed Track 0 and is named for its original SSH case. Its default is `rich`: Track 0 blocks render as inert content under the rules of this section. Its other value is `text`, under which all possibly-remote Track 0 renders as escaped text. The policy is read only from the user's policy file, as [Policy](@/spec/v0/capabilities.md#caps.policy) describes. A Lumen-aware multiplexer reached over SSH stays text only in v0, whatever this policy says ([remote multiplexers](@/spec/v0/multiplexers.md#mux.remote)).

An **SSH session** is a layer the terminal created for an SSH profile, or any layer whose foreground process tree contains an `ssh` client. When the terminal cannot tell whether a layer is an SSH session, it treats the layer as one. Under either value of `ssh.track0`, an image in Track 0 on an SSH session is not decoded. It shows its alt text as escaped text, whatever the state of the [decoder gate](@/spec/v0/conformance.md#gate.decoder), and it counts against the unsigned budget.

Only an explicit local policy set by the user in terminal settings lifts possibly-remote status, per profile: `track0.treat_unsealed_as_local = true`. Setting it shows a persistent warning in profile settings and a chrome notice on layers that use it. The lift changes how unsealed Track 0 renders under `ssh.track0`, but it does not change the image rule: an image in Track 0 on an SSH session is not decoded whatever that policy says. The lift never applies to multiplexer panes or to SSH layers created by the terminal. No signal in the output, environment or process tree ever lifts possibly-remote status. Sealed frames are classified by their layer.
