+++
title = "Platform support"
weight = 180
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Platform support {#platforms}

## Linux {#platforms.linux}

Linux supports default sealing (see [requirements](@/spec/v0/sealed-layers.md#sealed.requirements)) and optional [hardened mode](@/spec/v0/hardened-mode.md). Images are available after the decoder gates pass.

## macOS {#platforms.macos}

v0 creates no keys on macOS, so layers are grid plus Track 0. Images need the [decoder memory gate](@/spec/v0/conformance.md#gate.decoder-memory) and the [decoder gate](@/spec/v0/conformance.md#gate.decoder) to pass on macOS. Track B paths follow the macOS rule in [Transport](@/spec/v0/transport.md#transport.track-b).

## Windows {#platforms.windows}

Images are not supported on Windows in v0. The Windows console is a text channel: it reads the bytes a program writes as text in the console code page and re-encodes them as UTF-8, so the binary image frames (a kind byte, a big-endian length and the image bytes) do not arrive intact. The system pseudo-console also drops the escape string the frames travel in, and a pseudo-console shipped with the terminal carries that string but still changes the bytes inside it, with or without its pass-through mode. A Windows build carries no gate record, so it shows the alt text and never draws an image. Supporting Windows would need a frame encoding that survives text (for example 7-bit safe), which is a change to the wire format and not part of v0.

## Flatpak, Snap and rootless containers {#platforms.sandboxed}

- No Flatpak build is provided for v0.
- Snap and rootless containers are unsupported in v0. A terminal that detects it is running inside Flatpak, Snap or a non-initial user namespace creates no keys and shows a notice. Track 0 rendering follows the normal output rules.
- Immutable distributions (for example Fedora Silverblue and Steam Deck) are a follow-up after v0.

## Containers {#platforms.containers}

A shell running inside a container is just a PTY writer. Its output is unsealed and gets the Track 0 rules.

## tmux and screen {#platforms.tmux}

tmux and screen render Lumen output as escaped text in v0. See [Multiplexers](@/spec/v0/multiplexers.md#mux.tmux-screen).

## SSH {#platforms.ssh}

SSH sessions are grid plus Track 0, with no keys. See [Sealed layers](@/spec/v0/sealed-layers.md#sealed.ssh).
