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
| `format` | u8 | 1 PNG, 2 JPEG, 3 WebP; a hint that is never checked, since the terminal sets the format from the signature of the bytes. Any other value (0 or 4 to 255) is not malformed: it is ignored like a wrong hint |
| `width`, `height` | u32 each | the declared size, a hint only |
| `length` | u32 | the total encoded bytes that the data frames will carry, 1 to 16,777,216 |
| `alt` | string | the alt text, shown whenever the image is not |

**Image data (0x84).** The payload is the `id` (u32), the `offset` (u32) and then 1 to 262,144 bytes of the encoded image.

**Rules.**

- A producer has at most one image being received. A start frame that arrives while another image of that producer is incomplete first ends that image with its alt text, which frees its id, and only then is checked as below, so a start that reuses the id of the incomplete image is a fresh start and not a duplicate. An image is complete when the bytes received equal `length`, and then it is submitted to the decoder rules above.
- Data frames arrive in order. A data frame is accepted only for the incomplete image of its producer with the same `id`, and only when its `offset` equals the bytes received so far and its bytes do not pass `length`. Any other data frame (an unknown or finished id, a gap or overlap, an overrun) ends the incomplete image with its alt text, is shown as escaped text, and has no other effect. Nothing is skipped or reordered, and no declared length is trusted beyond the 1 MiB frame limit and the 16 MiB image limit. A data frame whose payload is truncated, has no bytes or has more than 262,144 is malformed: it is shown as escaped text with no other effect, and the incomplete image stays as it was, so the next data frame ends it if the bytes are now out of order.
- A start frame is checked in this order, and the first check that fails decides: (1) it is well formed, else it shows escaped text; (2) images are unlocked, the producer is not on an SSH session as an unsigned producer and is not degraded, else it shows its alt text and the locked rule below applies; (3) `length` is in range; (4) the size hints are not zero and are within [Input limits](@/spec/v0/images.md#images.input); (5) its `id` is not in use by the producer (waiting for a decode, decoding or shown; the image being received was ended above); (6) the 32 MiB held-bytes bound below. A start that fails a check from (2) on shows its alt text if it has a readable one and otherwise escaped text, and begins no image.
- An id stays in use while its image is waiting, decoding or shown. A shown image frees its id when the terminal drops the image (it leaves the scrollback, or the screen is cleared), and from then on the producer can use the id again. The end of the command does not drop images already shown.
- The encoded bytes a producer holds at once, in the image being received and in images waiting for a decode, are at most 32 MiB. A start frame that would pass it shows the alt text and degrades the producer as in [Memory budget](@/spec/v0/images.md#images.budget). The encoded bytes count with the producer's other budgets, including the 25% share of an unsigned producer.
- When images are not unlocked ([Platform unlock](@/spec/v0/images.md#images.unlock)), the start frame shows the alt text and the data frames of that id are dropped as they arrive, whatever their offset: no byte is buffered, and this rule wins over the ordering rule above. A data frame of any other id is an unknown id and falls under the ordering rule. The same holds for an unsigned image on an SSH session, and after a decoder crash for the rest of the command.
- Image frames count only inside the unverified-output container. Outside it, an image frame is shown as escaped text and nothing is kept for the strip.
- The image block is placed in the document where its start frame arrived. It shows the alt text until the decode finishes, then the decoded image at its decoded size, reduced to fit the width of its container and 40 rows of cells (proportions kept, never enlarged). The declared size does not set the layout.
- The end of the command, the end of the layer and a reset of the producer's stream end any incomplete image with its alt text and free its bytes.
- The image frame types have no sealed form in this profile. An image in a sealed layer needs the `images` capability and is not defined here.

## Platform unlock {#images.unlock}

Images are enabled on a platform only after the [decoder gate](@/spec/v0/conformance.md#gate.decoder) and the [decoder memory gate](@/spec/v0/conformance.md#gate.decoder-memory) pass. Both must pass for the decoder build identity and for the terminal's image-path build identity. A build identity is a hash of the sources, compiler, flags and the full dependency closure, including lockfile versions and hashes. Any change to an identity re-runs the gate, and the alt text is shown until it passes again. The local setting `images = off` is always available. There is no remote kill switch.

## Concurrency {#images.concurrency}

At most 1 decode is in flight per producer, and at most 2 across the terminal. Each producer can queue 8 images. Beyond that the alt text is shown. Queued images count against the producer's budget. After a decoder crash, that producer's images show alt text for the rest of the command.

## Memory budget {#images.budget}

Decoded images have a budget of 256 MiB per session. Each image reserves width × height × 4 bytes before any data is read. The terminal reads at most width × height × 4 + 4 KiB from the decoder. All size arithmetic is 64-bit and checked. Unsigned producers get the 25% share described in [MUST 4](@/spec/v0/trust.md#trust.must4-budgets). GPU textures are limited to 512 MiB in total.
