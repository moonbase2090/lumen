# The terminal's design system

The terminal's look is built from named tokens in `crates/lumen-term/src/design.rs`. This note says what the tokens are, the rules every set of values keeps, and which parts of the look use them today.

## Why tokens

The [text runs](../spec/v0/text-runs.md#runs.colour) section of the spec names colours by meaning (`fg`, `muted`, `accent`, `error`) and says the theme maps them to colours. The renderer had one fixed set of colours, named by where they were painted. Tokens name the roles once, so a theme is a choice of values for the same roles, and the floors below can be checked for every theme in one place.

## Colour roles

| Role | Used for |
|---|---|
| `canvas` | The page behind every block |
| `surface` | The container of a rich block, and the background-output strip |
| `sunken` | The background of code |
| `field` | The input editor |
| `chrome` | The trust margin: the gutter and the header strip |
| `text` | Body text; the `fg` run token |
| `text_strong` | Headings |
| `text_muted` | Labels, counts and hints; the `muted` run token |
| `chrome_text` | The terminal's own text in the trust margin |
| `accent` | The editor's prompt mark, links and focus; the `accent` run token |
| `error` | Errors and failed exits; the `error` run token |
| `caution` | The **unverified output** label: a caution, not an error |
| `control_border` | The edge of the input editor and of the unverified output container |
| `notice`, `notice_border` | A toast's box and border |
| `ansi[0..16]` | The sixteen ANSI colours of grid blocks |

## Floors every palette keeps

The tests in `crates/lumen-term/tests/design.rs` check these for each palette.

- **Text.** Every text role is at least 4.5:1 (WCAG AA for body text) against every surface it is drawn on. The spec's [visibility rule](../spec/v0/text-runs.md#runs.visible) treats a run under 3:1 as hidden, so the terminal's own colours never come near that line.
- **ANSI.** The colours programs use for text (0 to 6 and 8 to 14) are at least 3:1 against the canvas and the surface. 7 and 15 are the light greys by convention and are exempt.
- **Edges.** The control border is at least 3:1 against the surfaces on both sides of it, the WCAG floor for the boundary of a control. The edge of the unverified output container uses it, so that boundary is always clearly drawn.
- **Sizes.** Every step of the type scale is over the spec's 6 px floor for a visible run.

## Type

Runs have a family role (`body`, `heading` or `mono`) and a size step. The type scale is 12, 14, 17, 20 and 24 px at scale 1, with step 0 at 14 px, the body size; each step up is a ratio of about 1.2. Headings default to step 2. Body and mono default to step 0.

## What uses the tokens today

The window the terminal opens is drawn in the light palette. That covers every colour the renderer paints: the canvas, grid text and the ANSI palette, rich-block containers, code, headings, the trust margin, the editor and toasts. The editor's prompt mark is drawn in the accent, so the place to type is the one coloured mark in the chrome. Colour-query replies, when they are turned on, answer from the same palette, so a program sees the colours it is drawn in.

The renderer's `Theme` (colours by where they are painted) lives in the design module with the palette. Its fixed `Theme::light` is kept unchanged for the pinned conditions of the pixel evidence, so that baselines taken before this change stay comparable. Neither the palette nor the theme reads decoded image pixels, so neither is part of the image path's build identity, and changing a colour never moves it.

Not yet drawn from tokens: the type scale (the renderer has one bundled size), spacing and corner radii (the layout is on the cell grid). Those come with the renderer's proportional text layout and with theming: light, dark and high-contrast palettes, following the system appearance, and the user's choice of fonts and sizes.
