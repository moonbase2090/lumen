+++
title = "Images and the decoder"
weight = 120
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Images and the decoder {#images}

## Sandboxed decoder {#images.sandbox}

Images are decoded only by an out-of-process decoder. The decoder runs under seccomp or a platform sandbox profile, with a 512 MiB memory limit, 2 s of CPU per image, and no filesystem or network access. The terminal process never decodes images.

## Input limits {#images.input}

Only PNG, JPEG and WebP are accepted, at most 16 megapixels (16,000,000 pixels) and at most 65535 px on each side. The declared size is only a hint until the decoder has bounded it. The pixel limit is set so that the largest legal image, which reserves 64,000,000 bytes, fits within the [budget share](@/spec/v0/security.md#security.budget-shares) of one unsigned producer (25% of 256 MiB, 67,108,864 bytes) and so of any larger share. An image whose declared width times height is over the limit is refused before a decoder is started or any budget is reserved.

## Output format {#images.output}

The decoder returns the first frame only, as 8-bit premultiplied, top-down RGBA, with width, height and stride. Metadata is stripped inside the sandbox. No colour management and no orientation rotation are applied. On macOS the decoder bundles the same libraries and MUST NOT use the system ImageIO framework.

## Reply validation {#images.validate}

The terminal validates every decoder reply (width, height, stride, format and exact byte count) before it allocates or uploads anything. A mismatch shows the alt text and counts as a decoder crash.

## Image frames {#images.frames}

This section is part of the experiment profile and is not yet normative. An image reaches the terminal as one start frame and then one or more data frames, all unsealed ([Track 0 frames](@/spec/v0/transport.md#transport.track0-frames)). Integers are big endian, and a string is a length (u16) and that many bytes of UTF-8, at most 4096, as for the other Track 0 blocks.

**Image start (0x83).** The payload is, in order:

| Field | Size | Meaning |
|---|---|---|
| `id` | u32 | the image's id, scoped to the producer |
| `format` | u8 | 1 PNG, 2 JPEG, 3 WebP; a hint, since the terminal sets the format from the signature of the bytes |
| `width`, `height` | u32 each | the declared size, a hint only |
| `length` | u32 | the total encoded bytes that the data frames will carry, 1 to 16,777,216 |
| `alt` | string | the alt text, shown whenever the image is not |

**Image data (0x84).** The payload is the `id` (u32), the `offset` (u32) and then 1 to 262,144 bytes of the encoded image.

**Rules.**

- A producer has at most one image being received. A start frame while another image of that producer is incomplete ends that image with its alt text, and begins the new one. An image is complete when the bytes received equal `length`, and then it is submitted to the decoder rules above.
- Data frames arrive in order. A data frame is accepted only for the incomplete image of its producer with the same `id`, and only when its `offset` equals the bytes received so far and its bytes do not pass `length`. Any other data frame (an unknown or finished id, a gap or overlap, an overrun) ends the incomplete image with its alt text, is shown as escaped text, and has no other effect. Nothing is skipped or reordered, and no declared length is trusted beyond the 1 MiB frame limit and the 16 MiB image limit.
- A start frame that is malformed, has a `length` out of range, a size hint that is zero or over the limits of [Input limits](@/spec/v0/images.md#images.input), or an `id` the producer already used in this command, shows its alt text if it has a readable one and otherwise escaped text. It begins no image.
- The encoded bytes a producer holds at once, in the image being received and in images waiting for a decode, are at most 32 MiB. A start frame that would pass it shows the alt text and degrades the producer as in [Memory budget](@/spec/v0/images.md#images.budget). The encoded bytes count with the producer's other budgets, including the 25% share of an unsigned producer.
- When images are not unlocked ([Platform unlock](@/spec/v0/images.md#images.unlock)), the start frame shows the alt text and its data frames are dropped as they arrive: no byte is buffered. The same holds for an unsigned image on an SSH session, and after a decoder crash for the rest of the command.
- The image block is placed in the document where its start frame arrived. It shows the alt text until the decode finishes, then the decoded image at its decoded size, reduced to fit the width of the document. The declared size does not set the layout.
- The end of the command, the end of the layer and a reset of the producer's stream end any incomplete image with its alt text and free its bytes.
- The image frame types have no sealed form in this profile. An image in a sealed layer needs the `images` capability and is not defined here.

## Platform unlock {#images.unlock}

Images are enabled on a platform only after the [decoder gate](@/spec/v0/conformance.md#gate.decoder) and the [decoder memory gate](@/spec/v0/conformance.md#gate.decoder-memory) pass. Both must pass for the decoder build identity and for the terminal's image-path build identity. A build identity is a hash of the sources, compiler, flags and the full dependency closure, including lockfile versions and hashes. Any change to an identity re-runs the gate, and the alt text is shown until it passes again. The local setting `images = off` is always available. There is no remote kill switch.

## Concurrency {#images.concurrency}

At most 1 decode is in flight per producer, and at most 2 across the terminal. Each producer can queue 8 images. Beyond that the alt text is shown. Queued images count against the producer's budget. After a decoder crash, that producer's images show alt text for the rest of the command.

## Memory budget {#images.budget}

Decoded images have a budget of 256 MiB per session. Each image reserves width × height × 4 bytes before any data is read. The terminal reads at most width × height × 4 + 4 KiB from the decoder. All size arithmetic is 64-bit and checked. Unsigned producers get the 25% share described in [MUST 4](@/spec/v0/trust.md#trust.must4-budgets). GPU textures are limited to 512 MiB in total.
