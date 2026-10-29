+++
title = "Multiplexers"
weight = 140
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Multiplexers {#mux}

## Alternate screen {#mux.alt-screen}

While the alternate screen is active on a layer, unsealed Track 0 on that layer renders as escaped text.

## tmux and screen {#mux.tmux-screen}

If `TMUX` or `STY` is present in a layer's environment, Track 0 on that layer renders as escaped text. Support for tmux and screen is post-v0. Passthrough sequences render as text.

## Lumen-aware multiplexer {#mux.lumen-aware}

A Lumen-aware multiplexer MAY run as a sealed multiplexer layer. It is the only multiplexer that can show rich panes in v0, and rich panes are behind a setting until the [multiplexer gate](@/spec/v0/conformance.md#gate.multiplexer) passes.

### Keys {#mux.keys}

- The terminal delivers keys over an inherited fd only to a multiplexer client that the terminal spawned itself, from a profile or an Attach action, after checking its executable path.
- Keys stay in the client and never reach the multiplexer server. They are zeroised on detach and on exit.
- The client verifies its server with a peer-uid check (`SO_PEERCRED` or `getpeereid`). It opens the socket directory with `O_NOFOLLOW` and checks its owner and mode with `fstat`. If any check fails, all panes are text.
- Keys are scoped to exactly two sealed frame types: layout and pane container. Any other sealed type is rejected and treated as a desync, and the panes fall back to text.
- The multiplexer never relays OSC 52, titles or notifications.

### Pane containers {#mux.pane-containers}

- The multiplexer parses each pane's Track 0 with the Lumen parser. It re-emits the content as sealed pane-container frames that carry an opaque pane id, a layout generation and a rectangle.
- The outer terminal draws per-pane containers only from sealed layout and pane-container frames. It clips each container to its rectangle, drops stale generations and malformed rectangles, and collapses pane content during resize. Only visible panes are drawn.
- Only the outer terminal draws the trust margin. It draws a per-pane unverified strip.
- MUSTs 1 to 4 apply per pane. Decode limits stay terminal-wide. The 25% unsigned budget is split across the visible panes.
- Images in panes follow the same platform unlock as everywhere else: they are available once the decoder gates pass on that platform.
- The [alternate-screen rule](#mux.alt-screen) applies per pane.
- All pane Track 0 is possibly remote. It is classified when the multiplexer reads it, and the more remote of the read-time and emit-time states applies.

### Frame payloads {#mux.payloads}

This section is part of the experiment profile and is not yet normative. It gives the payloads of the two sealed frame types a multiplexer's keys cover: layout (type 0x01) and pane container (type 0x02), as numbered in [Transport and framing](@/spec/v0/transport.md). All integers are big endian. Coordinates are cells of the area the outer terminal gives the multiplexer layer, with the origin at its top left. A rectangle is `x`, `y` (signed 32-bit) and `w`, `h` (unsigned 32-bit).

A rectangle is **valid** if `x` and `y` are not negative, `w` and `h` are not zero, and the rectangle lies inside the layer's area.

**Layout frame (0x01).** The payload is a `generation` (u32), a `count` (u16) and `count` entries of a `pane id` (u32) followed by a rectangle, 20 bytes each. The entries are exactly the visible panes. A layout is **malformed** if `count` is over 256, the length does not match `count`, a pane id appears twice, a rectangle is not valid, or two rectangles overlap.

**Pane container frame (0x02).** The payload is a `pane id` (u32), a `generation` (u32), a rectangle, and then the pane content: one unsealed frame body, a type byte of 0x80 or above followed by its payload, which the terminal parses with the Track 0 parser under the limits of [Transport and framing](@/spec/v0/transport.md). A container is **malformed** if the payload is shorter than the 24-byte prefix, its rectangle is not valid, or the content type byte is below 0x80.

**What the terminal does.**

- It keeps one current layout per multiplexer layer, with a current generation. A new layer has no layout and no generation, and nothing is drawn.
- A layout takes effect only if it is well formed and its generation is greater than the current generation. A layout with an equal or lower generation is stale: it has no effect and is counted.
- A pane container is drawn only if its generation equals the current generation, its pane id is in the current layout, its rectangle equals the layout's rectangle for that pane, and its content parses. A container with a lower generation is stale. A container with a higher generation arrived before its layout. Both have no effect and are counted. A container for a pane that the layout does not list, or with a different rectangle, is dropped and counted.
- Stale and early frames are races, not faults, so they do not count toward the pin. A malformed layout or pane container is refused as a failed frame under [Frame abort](@/spec/v0/transport.md), has no effect, and counts toward the pin as a failed authenticated frame.
- A layout replaces the visible set: a pane that the new layout does not list is hidden and its containers are dropped until a later layout lists it.
- When the area of the multiplexer layer changes size, the terminal discards the current layout. No pane is drawn until a layout with a greater generation than any it has accepted arrives. A reattach starts a new layer, so it has no layout either.
- Every container is clipped to its rectangle, and no pane content is drawn outside it.

**Pane ids.** The multiplexer allocates pane ids from a counter that only increases, and never reuses an id for another pane. The terminal cannot tell a closed pane from a hidden one, so it enforces only that ids are unique within a layout; the never-reused rule is the multiplexer's obligation and a conformance test of the multiplexer checks it.

### Pane identity {#mux.pane-identity}

- In trust UI, a pane is identified by its position (window and pane index) plus a colour derived from its opaque id. A glyph is added when two colours are close.
- A "pane changed" marker is shown when a position is reused within 10 s.
- Pane ids are never reused.
- Pane titles, working directories and pane text never appear in trust UI.

### Reattach {#mux.reattach}

On reattach the client gets fresh keys. Only visible panes are replayed, from a bounded store. The outer terminal draws the "replayed" mark from a sealed flag.

### Remote multiplexers {#mux.remote}

A multiplexer reached over SSH is text only in v0.

### Unsealed multiplexer mode (informative) {#mux.unsealed}

An implementation MAY offer, off by default, a mode in which the multiplexer is treated as an ordinary unsealed producer. That mode gets only Track 0 rules and the possibly-remote treatment. The sealed multiplexer layer above is the default.
