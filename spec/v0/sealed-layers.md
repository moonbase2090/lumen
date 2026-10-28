+++
title = "Sealed layers and keys"
weight = 100
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Sealed layers and keys {#sealed}

A sealed layer is the only source of authority frames: command structure, exit status and granted capabilities. In v0, only the terminal-owned shell integration of a listed shell can hold a sealed layer. Applications cannot emit sealed frames.

## Default sealing requirements {#sealed.requirements}

The terminal creates keys for a layer only if all of the following hold:

1. **Per-layer AEAD key.** Each layer has its own keys. They are derived with HKDF-SHA256 from a per-layer root that never leaves the terminal, and the terminal-assigned layer id and the direction are bound into every derivation.
2. **Separate key holder.** Each layer has its own key-holder process. Every fd in the holder is close-on-exec except its seal fd and its control socketpair.
3. **Key delivery.** Keys are delivered only to the shell integration of a listed shell, over the inherited control fd. They never appear in the environment, in argv or on the PTY.
4. **Memory hygiene.** Key pages use `PR_SET_DUMPABLE=0`, `MADV_DONTDUMP` and `mlock`. This is hygiene, not a security boundary.
5. **Local session.** The session is local (not over SSH).
6. **Policy file checks.** The policy file passes its owner and mode checks (see [Policy](@/spec/v0/capabilities.md#caps.policy)).
7. **Side channel.** If Track B is used, its socket requires its token and accepts only sealed shell-integration frames.

If any requirement fails, no keys exist. The layer is then grid plus Track 0, with a chrome notice. Default sealing protects integrity against **output**. It does not protect against local malware running as the user; see the [threat model](@/spec/v0/threat-model.md) and [hardened mode](@/spec/v0/hardened-mode.md).

## Layer lifetime {#sealed.lifetime}

v0 has exactly one layer per terminal-spawned shell. Nested shells, attaching and plain multiplexers get no keys.

## Shim {#sealed.shim}

A Lumen shim starts each listed shell (for zsh, a `ZDOTDIR` shim; other listed shells use the equivalent). The terminal sets `LUMEN_SHELL_PID` to the spawned pid, which is not secret. The shim:

1. Proceeds only if the shell's own pid equals `LUMEN_SHELL_PID`.
2. Unsets `LUMEN_SHELL_PID`.
3. Restores or unsets `ZDOTDIR` (or the equivalent) for children.
4. Checks with `fstat` that the seal fd and control fd are Unix sockets.
5. Sets `FD_CLOEXEC` on both fds before it sources the user's own startup files.

If any check fails, the shim does nothing and touches no fd. A nested shell therefore never runs the shim. A shell with no shim gets no keys. No fd is received after start.

## Shell replacement {#sealed.exec-detector}

At the ready point, the terminal records the start time, `comm` and `argv[0]` of its own spawned shell. It re-reads them at every Enter it forwards and at every authenticated command-start frame. If any of them differ, or the shell pidfd is readable, the terminal shows a chrome notice and the layer runs without keys from then on. The detector only produces notices: it grants nothing and adds no sealing state. A same-name re-exec is not detected (see [residuals](@/spec/v0/threat-model.md#threat.residuals)).

## No mid-session rekey {#sealed.no-rekey}

v0 has no mid-session rekey. Narrowing a grant takes effect immediately. Widening a grant requires a new shell. Elevated grants are asked for, in chrome, when the shell starts.

## Control socketpair {#sealed.control}

The control channel between the holder and the shell integration is an `AF_UNIX` `SOCK_SEQPACKET` socketpair. Messages are at most 128 bytes and carry capability requests and replies only. The queue holds at most 16 messages, and sends use `MSG_DONTWAIT`, so a stalled peer cannot block the terminal.

## Terminal replies {#sealed.replies}

These rules apply on every layer, sealed or not.

- **Answered:** DA1, DSR 5 and CPR. CPR is generated from cursor state and has at most 5 digits per field. Colour queries OSC 4, 10, 11, 12, 17 and 19 are answered only in the single `?` query form. The reply comes from the theme only, in the fixed form `ESC ] N ; rgb:RRRR/GGGG/BBBB ST`, with at most 8 colour replies per PTY read chunk.
- **Rate:** at most 40 replies per second in total, in a sliding window, of which at most 8 may be non-colour replies.
- **Never answered:** DA2, DA3, DECXCPR, DECRQM, XTVERSION, XTWINOPS reports, kitty keyboard queries and ENQ (the answerback is empty).
- **Mouse and paste reports** are sent only for events the user caused, and only after a `tcgetpgrp` check of the foreground group. Mouse and bracketed-paste modes reset on alternate-screen exit, on RIS and on pin.

## SSH {#sealed.ssh}

v0 places no keys on SSH layers. SSH sessions are grid plus Track 0, and the [possibly-remote](@/spec/v0/trust.md#trust.possibly-remote) rules apply.
