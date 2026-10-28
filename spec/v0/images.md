+++
title = "Images and the decoder"
weight = 120
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Images and the decoder {#images}

## Sandboxed decoder {#images.sandbox}

Images are decoded only by an out-of-process decoder. The decoder runs under seccomp or a platform sandbox profile, with a 512 MiB memory limit, 2 s of CPU per image, and no filesystem or network access. The terminal process never decodes images.

## Input limits {#images.input}

Only PNG, JPEG and WebP are accepted, at most 32 megapixels and at most 65535 px on each side. The declared size is only a hint until the decoder has bounded it.

## Output format {#images.output}

The decoder returns the first frame only, as 8-bit premultiplied, top-down RGBA, with width, height and stride. Metadata is stripped inside the sandbox. No colour management and no orientation rotation are applied. On macOS the decoder bundles the same libraries and MUST NOT use the system ImageIO framework.

## Reply validation {#images.validate}

The terminal validates every decoder reply (width, height, stride, format and exact byte count) before it allocates or uploads anything. A mismatch shows the alt text and counts as a decoder crash.

## Platform unlock {#images.unlock}

Images are enabled on a platform only after the [decoder gate](@/spec/v0/conformance.md#gate.decoder) and the [decoder memory gate](@/spec/v0/conformance.md#gate.decoder-memory) pass. Both must pass for the decoder build identity and for the terminal's image-path build identity. A build identity is a hash of the sources, compiler, flags and the full dependency closure, including lockfile versions and hashes. Any change to an identity re-runs the gate, and the alt text is shown until it passes again. The local setting `images = off` is always available. There is no remote kill switch.

## Concurrency {#images.concurrency}

At most 1 decode is in flight per producer, and at most 2 across the terminal. Each producer can queue 8 images. Beyond that the alt text is shown. Queued images count against the producer's budget. After a decoder crash, that producer's images show alt text for the rest of the command.

## Memory budget {#images.budget}

Decoded images have a budget of 256 MiB per session. Each image reserves width × height × 4 bytes before any data is read. The terminal reads at most width × height × 4 + 4 KiB from the decoder. All size arithmetic is 64-bit and checked. Unsigned producers get the 25% share described in [MUST 4](@/spec/v0/trust.md#trust.must4-budgets). GPU textures are limited to 512 MiB in total.
