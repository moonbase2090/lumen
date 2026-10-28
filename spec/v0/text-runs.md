+++
title = "Text runs and styling"
weight = 40
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Text runs and styling {#runs}

## Runs {#runs.definition}

A run is UTF-8 text plus a style: family role (body, mono or heading), weight, italic, size step, colour token, underline, strike and link reference.

## Colour {#runs.colour}

Colour tokens are semantic (`fg`, `muted`, `accent`, `error`), and the theme maps them to colours. Raw RGB is allowed only as a capability.

## Cell spans {#runs.cell-spans}

A run of N cells can hold a shaped proportional string fitted to that width, for use inside grids. Cell spans MUST be created only by authenticated frames, never by grid output. Unverified output cannot create them.

## Shaping {#runs.shaping}

Shaping is HarfBuzz-style, with ligatures, emoji and bidi. Applications never measure text.

## Copy semantics {#runs.copy}

Copy is defined as the logical code-point sequence of visible runs, not the shaped glyphs. When shaped glyphs differ from code points, the code points are copied.

## Bidi and invisible characters {#runs.bidi}

In command and code blocks, and in pasted command text, the terminal MUST strip the following and show how many were stripped:

- Bidi override, isolate and mark controls: U+202A–U+202E, U+2066–U+2069, U+200E, U+200F and U+061C.
- In command text only: zero-width and invisible characters U+200B–U+200D, U+2060–U+2064 and U+FEFF, and tag characters U+E0000–U+E007F.

Elsewhere these characters are rendered visibly as escapes. Command input is displayed in logical order, with bidi reordering disabled, so the command that is displayed is the one that runs.

## Visibility {#runs.visible}

A run is visible only if all of the following hold:

- Its resolved size is at least 6 px.
- Its foreground/background contrast ratio is at least 3:1 (WCAG relative luminance).
- It is not clipped or scrolled out of its block.
- Its section is expanded.
- No part of it is covered by any later paint, opaque or not: another run, a cell span, an image or a block. If coverage cannot be computed, the run is treated as covered (fail closed).

A run that fails any test is treated as hidden for copy, search and approvals, and is counted in the hidden-content indicator. Hidden runs never reach copy.
