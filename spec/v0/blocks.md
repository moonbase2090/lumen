+++
title = "Block types"
weight = 30
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Block types {#blocks}

Any producer can emit the inert block types listed as Track 0 in [Unverified output](@/spec/v0/trust.md#trust.track0-scope). Those blocks always render inside an unverified output container. Command structure is reserved for sealed shell integration.

| Block | Purpose and rules |
|---|---|
| `paragraph` | Flowing proportional text runs. |
| `heading` | Levels 1 to 4, shown in the outline. Outline entries from unverified output carry the unverified label. |
| `table` | Real-layout table with column sizing and alignment, subject to the [table limits](@/spec/v0/layout.md#layout.limits). |
| `image` | PNG, JPEG or WebP by id, with alt text and an intrinsic size. Ids are scoped per producer. Images are never referenced from grids. The declared size is only a hint and never drives allocation or layout before a bounded decode. See [Images](@/spec/v0/images.md). |
| `link` | Inline or block. See [Links](@/spec/v0/links-and-clipboard.md#links). |
| `section` | Collapsible container with a summary line. Collapsed children are excluded from copy and search until they are expanded. The summary shows a count of hidden content. |
| `code` | Monospace text with an optional language tag. Mixed-script tokens (for example Latin mixed with Cyrillic or Greek, per UTS 39) are flagged, and an ASCII-only copy is offered. Code blocks and copy-code buttons get the same confusable check and badge. |
| `command` | Prompt, input and output children, plus exit status. Created only from sealed shell-integration frames. Input tokens with mixed scripts or confusables are flagged before they run and on copy, and an ASCII-only copy is offered. |
| `grid` | Legacy VT surface with its own cols × rows on a PTY. See [Grid blocks](@/spec/v0/grid.md). An OSC title never names the producer in chrome. |
| `progress` / `status` | Live-updating, patchable widgets. Content only: they MUST NOT render permission prompts or trust badges, and the chrome shows their producer. |
