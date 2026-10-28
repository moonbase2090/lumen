+++
title = "Lumen Protocol v0"
weight = 0
sort_by = "weight"
template = "spec-section.html"
page_template = "spec-page.html"
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Lumen Protocol v0 {#lumen-v0}

**Lumen Protocol v0, revision 25 (frozen snapshot)**

- Copyright © Moonbase 2090 LLC. Moonbase 2090 LLC is the publisher.
- This specification is licensed under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Reference code is licensed MIT OR Apache-2.0.
- **Security issues:** report them privately through GitHub private vulnerability reporting on this repository, or by email to security@lumenprotocol.org. Do not open public issues for vulnerabilities.

This is a frozen snapshot. Later changes will be published as new revisions or versions. They will not edit this text.

## Summary {#summary}

A Lumen session is a document made of blocks, not one character grid. Rich blocks use proportional fonts and browser-style layout. Grid blocks host legacy VT applications on a PTY sized to the block, so old and new applications share one document. Lumen is deliberately not backwards compatible, but grid blocks give legacy applications a built-in middle path.

The core idea is that the terminal becomes a typeset document. The VT grid stays as one element type in it, much like an embedded `pre` element in a web page.

Bright Term is the reference terminal. Lunatui is the first native client library.

## Conformance language {#conformance-language}

The key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are to be read as described in RFC 2119 and RFC 8174 when they appear in capitals. Sections marked *informative* are not normative. Numeric limits are normative for v0 unless they are marked as proposed starting values.

## Goals {#goals}

1. **Secure by default (prime directive).** Untrusted output never gains authority, meaning protocol powers. Rendering inert content inside an unverified output container is not authority. Every power is off until it has been negotiated and authenticated.
2. Structured content as a protocol: blocks, text runs, layout and events.
3. Proportional text, real tables, images, links and collapsible sections.
4. Legacy VT applications run unchanged inside grid blocks.
5. Each command and its output is a block, not grid scrollback.
6. GPU rendering without a web engine.
7. Predictable copy and paste, and hit testing down to character offsets.
8. An open specification with a reference terminal and a conformance suite.

## Non-goals {#non-goals}

- Byte-for-byte backwards compatibility with xterm outside grid blocks.
- Embedding HTML, CSS or JavaScript.
- Multiplexer integration in v0 beyond a Lumen-aware multiplexer that implements the [sealed multiplexer layer](@/spec/v0/multiplexers.md#mux.lumen-aware). tmux and screen are post-v0.
- Remote GUI or general application streaming.
- Running in a browser in v0.

## Contents {#contents}

1. [Prior art](@/spec/v0/prior-art.md) (informative)
2. [Document model](@/spec/v0/document-model.md)
3. [Block types](@/spec/v0/blocks.md)
4. [Text runs and styling](@/spec/v0/text-runs.md)
5. [Layout model](@/spec/v0/layout.md)
6. [Events and input](@/spec/v0/events.md)
7. [Grid blocks and PTY sizing](@/spec/v0/grid.md)
8. [Transport and framing](@/spec/v0/transport.md)
9. [Unverified output (Track 0) and the trust margin](@/spec/v0/trust.md)
10. [Sealed layers and keys](@/spec/v0/sealed-layers.md)
11. [Capability negotiation and versioning](@/spec/v0/capabilities.md)
12. [Images and the decoder](@/spec/v0/images.md)
13. [Links, clipboard, copy and paste](@/spec/v0/links-and-clipboard.md)
14. [Multiplexers](@/spec/v0/multiplexers.md)
15. [Security considerations and resource limits](@/spec/v0/security.md)
16. [Threat model and accepted residual risks](@/spec/v0/threat-model.md)
17. [Optional hardened mode](@/spec/v0/hardened-mode.md)
18. [Platform support](@/spec/v0/platforms.md)
19. [Conformance gates](@/spec/v0/conformance.md)
20. [References](@/spec/v0/references.md) (informative)

## Review {#review}

This specification went through repeated independent adversarial security reviews before it was frozen. At freeze, no Critical, High or Medium findings remained open. The remaining Low and Informational items that the maintainers accepted are summarised in [Accepted residual risks](@/spec/v0/threat-model.md#threat.residuals).
