+++
title = "Grid blocks and PTY sizing"
weight = 70
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Grid blocks and PTY sizing {#grid}

## Layout element {#grid.element}

A grid block has its own cols × rows, like an embedded `pre` element.

## PTY sized to the block {#grid.pty-size}

A command in a grid block runs on a PTY whose winsize is the block's cell size, not the window's.

## Alternate screen {#grid.alt-screen}

An alternate-screen application grows its block to the viewport. On exit, the block collapses back to a static snapshot. Restoring an alternate screen uses an inert snapshot or a sanitised state replay with no OSC 52, OSC 133, OSC 8 or mode side effects. Raw VT bytes are never fed in again.

## Resize {#grid.pty-resize}

Every resize sends SIGWINCH with the block's new cell size. Grid winsize is capped at 1000 cols × 500 rows. Resizes are coalesced, with at most 20 SIGWINCH per second per grid.

## Commands are blocks {#grid.commands}

Each command and its output is a block, not grid scrollback. Boundaries come only from authenticated frames sent by the terminal-owned shell integration, or from the terminal's labelled [unverified boundaries](@/spec/v0/trust.md#trust.unverified-boundaries). OSC 133 inside a grid is visual decoration only: it creates no blocks and no exit status, and `aid` nesting is ignored.

## Grid parser isolation {#grid.parser-isolation}

- In grid mode the parser MUST drop all string-type escapes (DCS, APC, PM, SOS) and all C1 controls, both as raw bytes 0x80–0x9F and as decoded code points U+0080–U+009F (for example UTF-8 `C2 9B`), whichever the parser matches on.
- OSC is limited to a small allowlist (title and colours). OSC 8, OSC 52 and OSC 133 have no protocol effect in grids.
- The only string terminator is ESC `\` (and BEL for allowlisted OSC). Every string, including a dropped DCS/APC/PM/SOS that is being skipped, has a hard cap of 4 KiB. At the cap, the parser enters a skip state that discards bytes until ESC `\` or a grid reset and shows a "string truncated" notice. Parsing never resumes on the tail of a payload.
- The frame introducer is never recognised by grid-mode code. Only the frame layer, outside any grid, looks for it.
- Allowlisted OSC title text is shown only in a labelled app-title field or not at all, and never as the producer name.

## Grid sandbox {#grid.sandbox}

VT output inside a grid can never emit Lumen frames, reference images or reach the decoder. The conformance fuzzer feeds every introducer variant (7-bit, 8-bit C1, UTF-8-encoded C1 and string types) at every split position. It requires that no frame is accepted from bytes that lack valid authentication under a held key, and that no bytes of an accepted frame reach the VT parser.

## Grid input handling {#grid.input}

Grid shell input and grid text get the same bidi and invisible-character handling as command text (the command and code text rule of [Bidi and invisible characters](@/spec/v0/text-runs.md#runs.bidi), not the paragraph rule).
