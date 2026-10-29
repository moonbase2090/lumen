+++
title = "Conformance gates"
weight = 190
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Conformance gates {#gates}

A gate is a test suite that an implementation must pass before a feature is enabled on a platform. A failing gate disables only the feature it covers. Results apply to a specific build identity.

## Track 0 gates {#gate.track0}

Track 0 stays off until all three of these pass:

- <a id="gate.trust-margin"></a>**Trust margin**: under adversarial Track 0 input, the pixels of the trust margin stay unchanged.
- <a id="gate.no-authority"></a>**No authority from unsealed input**: no unsealed input creates command structure or exercises any capability. The one exception is the user's `allow_unsigned` policy opt-in, which lets unsigned grid output on a local layer draw a terminal toast, at most one per layer per second ([Notifications](@/spec/v0/trust.md#trust.notifications)).
- <a id="gate.parity"></a>**Side-channel parity**: no Lumen channel accepts anything that PTY writes could not already do.

## Decoder memory gate {#gate.decoder-memory}

The terminal's memory use stays within its budgets and reservations even when the decoder is hostile and lies about sizes, formats or byte counts.

## Decoder gate {#gate.decoder}

For each image format the terminal accepts, the decoder and the terminal's image path MUST meet these obligations, each of which a test can show:

- **Bounded resources.** No input makes the decoder run longer than 2 s or use more than 512 MiB. A hang over 2 s and memory use over 512 MiB count as crashes.
- **Validate before display.** The terminal checks every decoder reply (width, height, stride, format and exact byte count) before it allocates or uploads anything, as [reply validation](@/spec/v0/images.md#images.validate) says. A reply that fails the check is discarded and the alt text is shown.
- **Reject malformed input.** A malformed, truncated, oversized or lying input is refused with no output and no crash.
- **Fail safely.** A decoder crash, a hang, an over-limit reply or a failed check never takes the terminal down and never shows partial output. That producer's images show alt text for the rest of the command, as [concurrency](@/spec/v0/images.md#images.concurrency) says.
- **Sandbox escape tests pass.** The named test set passes in full.

A crash, a hang over 2 s, memory use over 512 MiB, or a report from the memory and undefined-behaviour checking that the decoder's language provides, found by any test, blocks release, and the fix ships as a patched build. For a decoder written in a memory-safe language that checking is the language's own: overflow and assertion checks left on, and a memory sanitizer where one exists. UBSan is not applicable to such a build, because it has no counterpart there, and the gate does not require it. The gate covers both the decoder build identity and the terminal image-path build identity.

*Non-normative.* Coverage-guided fuzzing of each format, under a memory sanitizer where one is available and with the language's overflow and assertion checks on, is the recommended way to look for violations of these obligations. How long to fuzz, and when to stop, is a release decision and not a conformance requirement; an implementation's release criteria state it. Good seeds are inputs from past vulnerabilities. Public sources include the vulnerability database entries and published reproducers for the reference libraries of each format (for example, the libwebp WebP heap overflow of 2023, CVE-2023-4863), the public fuzzing corpora that the reference libraries' own projects publish, and the regression test suites that the decoder libraries themselves keep for the bugs they fixed.

## Multiplexer gate {#gate.multiplexer}

This gate covers the [Lumen-aware multiplexer](@/spec/v0/multiplexers.md#mux.lumen-aware) rules: key custody and scope, pane-container clipping, stale-generation handling, pane identity, reattach and fallback to text. Rich panes stay behind a setting until it passes.

## Sealing gates {#gate.sealing}

- **Memory locking:** key pages stay resident and excluded from dumps.
- **Holder death:** when a holder dies, its layer fails closed to grid plus Track 0 with a notice.

## Hardened-mode gates {#gate.hardened}

- **Linux:**
  - Same-uid code cannot attach to, or open perf events on, the terminal or holders.
  - Fd inheritance, shim and shell-replacement cases behave as specified.
  - Startup and install checks fail closed for user-readable, dynamically linked or user-writable installs.
  - The latch holds.
  - At most 40 replies are sent per second.
- **macOS (for a future hardened mode):** task-port access is denied, and crash reports are scanned for key bytes.

## Remote keys (future) {#gate.remote}

Keyed SSH layers are not part of v0. Any future design requires its own gate.

## Fuzzing {#gate.fuzzing}

The conformance fuzzer feeds every introducer variant at every split position. It requires that no frame is accepted without valid authentication and that no bytes of an accepted frame reach the VT parser. Every numeric limit in this specification has a test vector.
