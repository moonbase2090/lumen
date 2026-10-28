+++
title = "Layout model"
weight = 50
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Layout model {#layout}

## Flow {#layout.flow}

Blocks are laid out in vertical flow, like a browser's normal flow, with inline layout inside paragraphs. Line breaking follows Unicode UAX 14, and content reflows when the width changes.

## Tables {#layout.tables}

Tables use a simple automatic column algorithm (min/max content). There are no floats and no absolute positioning in v0.

## Grid blocks {#layout.grids}

Grid blocks are fixed-size replaced elements: cols × rows × cell metrics.

## Viewport {#layout.viewport}

The viewport scrolls smoothly and is virtualised, so only visible blocks are laid out.

## Limits {#layout.limits}

- Tables have at most 256 columns and 100,000 cells. Column auto-layout is bounded to linear passes.
- Shaped-run caches are LRU with a byte budget of 64 MiB. They do not grow per width.
- Reflow on resize is debounced. The grow/collapse feedback loop MUST be fuzzed for oscillation.
