+++
title = "Security considerations and resource limits"
weight = 150
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Security considerations and resource limits {#security}

## Principles {#security.principles}

- **Least authority.** Every power is off until it has been negotiated, authenticated and granted.
- **Inert by default.** Unverified output is rendered, never obeyed.
- **Out-of-band trust UI.** Confirmations and badges live in terminal chrome and the trust margin, which content can never paint.
- **Fail closed.** When a check cannot be completed, the terminal takes the less powerful option: text instead of rich content, alt text instead of an image, no keys instead of keys.
- **Parity.** No Lumen asset gives more than the PTY-write path or the user's own dotfiles already give. See [Parity](@/spec/v0/threat-model.md#threat.parity).

## Parser hardening {#security.parsers}

Every parser that reads untrusted bytes (frame detector, VT parser, payload decoder, shaping inputs) is bounded before any C or FFI boundary and is fuzzed continuously.

## Unsafe and FFI boundary {#security.unsafe-boundary}

- **Safe Rust only (`#![forbid(unsafe_code)]`):** the frame decoder, handshake, key handling, document store, layout and hit testing.
- **Audited and fuzzed:** the VT core is audited for unsafe code and fuzzed.
- **Named C or FFI modules, outside the no-unsafe claim:** text shaping (HarfBuzz), font loading and rasterisation, GPU drivers and graphics bindings. These receive only input that safe code has already validated and bounded.
- **Out of process:** image decoding.

The implementation ships a dependency list that marks each C dependency and where FFI sits.

## Resource limits {#security.limits}

| Resource | Limit |
|---|---|
| Blocks per document | 100,000 |
| Patches per block | 10,000 per second |
| Patches per document | 50,000 per second |
| Shaping cache | 64 MiB |
| Table size | 256 columns, 100,000 cells |
| Grids per document | 64 |
| Grid cells per document | 4 million |
| Scrollback per document | 64 MiB |
| Grid winsize | 1000 × 500 |
| Frames per producer | 2,000 per second |
| Bytes per producer | 8 MiB per second |
| Frame size | 1 MiB (16 MiB for images on Track B) |
| Payload nesting depth | 32 |
| Decoded images | 256 MiB per session |
| GPU textures | 512 MiB |
| Layers | at most 8 |

## Budget shares {#security.budget-shares}

- At least 25% of every budget is reserved for local use and is never available to producers.
- No producer may use more than 50% of the remainder.
- Unsigned producers get 25% of each budget (see [MUST 4](@/spec/v0/trust.md#trust.must4-budgets)).
- Under a Lumen-aware multiplexer, that 25% is split across the visible panes.
- When a producer goes over budget, it falls back to plain grid text for the rest of the command, with a chrome notice. There is no throttle loop.

## Release hygiene {#security.release}

Release builds MUST pass an automated scan for personal identifiers, and a finding blocks the release. The only allowlisted string is the publisher name, "Moonbase 2090 LLC".
