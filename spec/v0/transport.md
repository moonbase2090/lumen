+++
title = "Transport and framing"
weight = 80
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Transport and framing {#transport}

## Tracks {#transport.tracks}

Lumen has two kinds of content on the PTY:

- **Track 0 (unverified output).** These are inert rich blocks in unsealed frames, which any program may write. They carry no authority. See [Unverified output](@/spec/v0/trust.md).
- **Sealed frames (Track A).** These are framed escape sequences on the PTY, authenticated with per-layer keys that are held outside the shell (see [Sealed layers](@/spec/v0/sealed-layers.md)). There is no session-wide token.

A **side channel (Track B)**, such as a local Unix socket negotiated for a sealed layer, is an optional upgrade. The PTY is the baseline transport.

Frame payloads use a compact binary schema. A JSON debug form exists for tooling only and is non-normative (see [Debug form](#transport.debug-form)).

## One pipeline {#transport.pipeline}

PTY bytes pass through the frame detector and then the VT parser. The frame detector sees every byte first, removes the bytes of every accepted frame from the stream, and passes all other bytes to the VT parser. A frame is accepted once its header is accepted, and its bytes MUST never reach the VT parser, whether or not its body then verifies. A candidate whose header is rejected is not a frame: its introducer is withheld, its header is shown as escaped text, and the rest continues as ordinary output, under the rule in [Frame abort](#transport.abort).

## Detector {#transport.detector}

A single detector state machine runs on the raw stream ahead of the VT parser. Its state persists across all PTY reads, including a partial introducer. The VT parser's per-read reset does not apply to it. A split introducer is reassembled and then its header must still be accepted (verified, for a sealed frame). The introducer is a sequence that the grid-mode VT parser never recognises, and it includes a 64-bit fixed tag so binary output almost never matches it. When a candidate is not an accepted frame, its introducer is withheld and never reaches the VT parser. Its header and the rescan of the bytes after it follow the rule in [Frame abort](#transport.abort). A raw 0x9F byte that continues an incomplete UTF-8 sequence is text, not an introducer, so the 8-bit C1 introducer is recognised only outside a UTF-8 sequence.

## Track 0 frames {#transport.track0-frames}

Track 0 frames are length-prefixed framed escapes on the PTY. They use the same header-first, split-safe detector, 4 KiB string caps and introducer fuzzing, without AEAD. A Track 0 header is never verified, so its declared length delimits the payload only up to the frame size limit and never skips input the frame does not contain; it cannot claim a region as a failed frame. An unsealed frame has its own type byte. A sealed-frame verifier MUST never accept an unsealed type, and an unsealed parser MUST never accept a sealed type (domain separation). A malformed, truncated or unknown Track 0 frame renders as escaped text and has no other effect. There is no handshake: Track 0 is enabled by terminal policy and advertised with `LUMEN=0`, which means "Track 0 only" and does not mean disabled. Per-host and per-layer policy can narrow it, for example `ssh.track0 = text-only`.

## Sealed frame header {#transport.header}

Every sealed frame has one fixed-size header layout. Its 64-byte authenticator field carries a 128-bit tag, zero-padded, and the padding is checked. The tag covers the layer id, direction, sequence number, type and length. It is computed under a header subkey derived from the layer's app key and is verified **before** any payload byte is consumed. The header contains a reserved origin byte. In v0 only app origin is valid.

## Sealed frame payload {#transport.aead}

Every sealed payload is protected with an AEAD under the derived key. The nonce is direction || 64-bit sequence number. The associated data covers layer id, capability, direction, origin, sequence number, type and length. This means captures (for example `script` or multiplexer pipe output) hold only ciphertext and headers. Sensitive content such as clipboard text and approvals MUST travel only in sealed frames. The cipher choice (ChaCha20-Poly1305 or AES-256-GCM) MUST be constant-time on all target CPUs, and nonces are never reused.

## Sequence numbers {#transport.sequence}

Sequence numbers are scoped per (layer id, direction), are shared by all capabilities on the layer, and are strictly increasing. Gaps are allowed. A replayed or stale sequence number is rejected.

## Limits {#transport.limits}

Decoders MUST cap every allocation from a declared length before reserving memory. The frame size limit is 1 MiB, or 16 MiB only for images on Track B. Nesting depth is limited to 32, and indefinite-length items are not allowed. Every dependency in the decode path MUST be pinned and audited for unsafe code.

## Frame abort {#transport.abort}

- An invalid header, an AEAD failure or a verified length beyond the limits aborts only that frame. A declared length over the 1 MiB frame size limit makes its header rejected, sealed or not. Beyond that limit, the declared length of an unverified header is never trusted to skip or swallow input, and only a verified header's length applies the other caps.
- After a rejected header, the layer creates no parser state of its own. The introducer is withheld from the VT parser. The header is the 13 bytes after the introducer (the tag, type byte and length field), fewer if the stream ends or an introducer starts among them. Those bytes are shown as escaped text, charged to the unauthenticated budget, and are not also scanned as output: from any introducer among them the normal rescan applies, so a real frame there is still found. Everything after the 13 bytes continues to the grid parser as ordinary output, subject to string caps.
- A verified header authenticates its length, so a body that fails AEAD aborts exactly that frame. The claimed region is rendered once as a failed frame, and the detector resumes after that introducer, so an authentic frame inside the region is still found.
- A failed frame (a verified header whose body fails) renders as at most 256 bytes of escaped literal text in a visually marked span (ESC shown as a visible glyph such as ␛, and all controls shown as escapes), plus a byte count. The rest is dropped. A failed frame is never parsed as VT and never forms a block.
- Verification work is capped per layer per second at 4096 header checks and 256 body AEAD checks. A candidate beyond a cap is not verified and follows the rejected-header rule above, and its bytes are charged to the unauthenticated bucket.
- Nothing is charged to a producer before its header verifies. Bytes that look like frames but are unverified go to one capped per-layer unauthenticated bucket.
- A frame aborts if its byte rate falls below 16 KiB/s after the first 2 s.
- A command-start frame closes any open block. A block with no exit frame shows as ended-unknown, never as success.

## Pin to grid mode {#transport.pin}

A single abort has no effect on layer state. The only desync is the rate pin: more than 3 aborts of authenticated (sealed) frames on a sealed layer within 60 s pins that layer to grid mode with a chrome notice, and open blocks close as ended-unknown. Malformed Track 0 frames, unsealed type bytes and frames from unsealed or remote sources never count toward the pin. Neither does any frame that fails header verification, including one rejected for a replayed or stale sequence number or because it is past a verification cap, so unauthenticated or replayed input cannot pin a layer. The chrome offers an out-of-band "restore Lumen" action, and only the user's confirmation starts a fresh handshake with fresh keys. An abort never triggers a rekey or re-handshake by itself.

## Recovery markers {#transport.recovery}

Recovery markers MUST be authenticated. The terminal never scans for unauthenticated resync markers.

## Side channel (Track B) {#transport.track-b}

- **Socket location.** On Linux the socket lives in `$XDG_RUNTIME_DIR/lumen/`. If that variable is unset, Track B is refused, never placed in a shared temporary path. On macOS it lives in the per-user temporary directory, in a short random subdirectory created with mode 0700 and verified with `lstat` (owner is the user, mode 0700, not a symlink). The full path MUST be under 104 bytes. A pre-existing or squatted path is refused and a new random name is chosen.
- **Peers.** A connection is accepted only if its first message proves the layer's one-time 256-bit Track B token (a MAC over a terminal nonce) and its peer credentials match the key holder spawned for that layer. A matching pid or uid alone is never enough. The token is delivered only to the layer's key holder and never enters the shell.
- **Content.** The holder sends over the side channel only frames that the shell requested over the seal channel, byte for byte, within the frame limit, and relays nothing else.
- **Ordering.** The PTY is the ordering authority. Side-channel messages carry one-shot ids and take effect only when an authenticated PTY marker that references them arrives. The marker carries a hash of the staged content. A second message for a staged id is rejected. The marker buffer is limited to 4 MiB and 256 pending ids, with a 5 s timeout. On timeout the pending block fails inert. There is no inline fallback.
- **Fallback.** If the side channel drops, the layer falls back to the PTY. The fallback is a visible chrome state change and requires a fresh handshake.
- **Parity.** The side channel and every other Lumen channel MUST accept nothing that PTY writes could not already do.

## Debug form {#transport.debug-form}

The JSON debug form is for tooling only. It MUST NOT be produced on any path that carries live keys, and debug output redacts tags and keys.
