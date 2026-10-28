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
