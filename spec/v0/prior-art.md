+++
title = "Prior art"
weight = 10
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Prior art (informative) {#prior-art}

| System | Lumen borrows | Lumen differs |
|---|---|---|
| Kitty graphics | Image IDs, chunked transfer, placement kept apart from data | Images are first-class blocks, not cell overlays sent through APC escapes |
| iTerm2 inline images | Simple inline images with size hints | No base64 inside OSC; framed binary payloads instead |
| Warp blocks | Command plus output as one block that can be selected or folded | Blocks are part of the protocol. Authority frames come only from terminal-owned shell integration; any program can emit inert blocks as unverified output |
| Notcurses | Planes, compositing, capability detection | The terminal does the layout; the application does not draw cells |
| DomTerm | Terminal as a document; prompt, input and output marks; folding | No DOM or web engine; a smaller typed schema |
| Jupyter | Typed cells, MIME bundles, kernel and front-end split | Streams in real time over a PTY-like channel, not notebook files |
| Arcan TUI | Clean break from VT; structured client API | Keeps VT as a native grid block instead of dropping it |
| OSC 133 | Prompt, input, output and exit-code semantics | Becomes structured block events sent only as authenticated frames by terminal-owned shell integration. OSC 133 inside grids is visual decoration only |
