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

## Built-in palettes

There are three: **light**, **dark** and **high contrast** (black on white). By default the terminal follows the system's appearance, light or dark, and switches the moment the system does, with no restart. A user can instead fix one palette whatever the system shows. Colour-query replies, when they are turned on, always answer from the palette on screen.

## Floors every palette keeps

The tests in `crates/lumen-term/tests/design.rs` check these for each built-in palette, and `Palette::check` reports each miss.

| | Light and dark | High contrast | A theme file |
|---|---|---|---|
| Text roles on every surface | 4.5:1 | 7:1 | 3:1 |
| ANSI text colours on the canvas and the surface | 3:1 | 4.5:1 | not checked |
| The control border beside it | 3:1 | 3:1 | not checked |

- **Text.** 4.5:1 is WCAG AA for body text. The spec's [visibility rule](../spec/v0/text-runs.md#runs.visible) treats a run under 3:1 as hidden, so a theme file is refused if any of its text falls under that.
- **ANSI.** The black and white slots that sit next to the background by convention are exempt: 7 and 15 on a light palette, 0 on a dark one. The readable one of black and white is at least 4.5:1.
- **Edges.** The edge of the editor and of the unverified output container use the control border, so that boundary is always clearly drawn.
- **Sizes.** Every step of the type scale, and every font size a user may choose (8 to 72 px), is over the spec's 6 px floor for a visible run.

## Theme files

A theme file is the small part of TOML a palette needs. It starts from a built-in palette and sets any colour role (the names in the table above) or ANSI colour as a quoted `#rrggbb` value; a role it does not set keeps the base's colour.

```toml
# A dark theme with a green accent.
base = "dark"          # light, dark or high-contrast; light if not given
accent = "#7ee2a8"
ansi.2 = "#7ee2a8"     # ansi.0 to ansi.15
```

The parse is strict. An unknown key, a key given twice, a value that is not a quoted `#rrggbb` colour, or a line that is not `key = value` refuses the whole file and names the line, because a half-read theme is worse than none. A file over 16 KiB is refused, and so is a theme whose text would be hidden (above).

## Fonts

A user can choose the face and the size of monospace text (grids, code and the editor) and of prose (paragraphs and headings in rich blocks), separately. A face is the bundled one or a font file; a file that is not a font with Latin letters, or is over 32 MiB, is refused. Sizes run from 8 to 72 px. A cell is as wide as the monospace `M`, and a row is tall enough for a line of either face, so prose set larger than the grid is not cut. A change takes effect at once: the cells are measured again and the program's grid is resized to fit the window.

## Type

Runs have a family role (`body`, `heading` or `mono`) and a size step. The type scale is 12, 14, 17, 20 and 24 px at scale 1, with step 0 at 14 px, the body size; each step up is a ratio of about 1.2. Headings default to step 2. Body and mono default to step 0.

## What uses the tokens today

The window is drawn in the palette the theme choice and the system's appearance give. That covers every colour the renderer paints: the canvas, grid text and the ANSI palette, rich-block containers, code, headings, the trust margin, the editor and toasts. The editor's prompt mark is drawn in the accent, so the place to type is the one coloured mark in the chrome.

The renderer's `Theme` (colours by where they are painted) lives in the design module with the palettes. Its fixed `Theme::light` is kept unchanged for the pinned conditions of the pixel evidence, and the default fonts give the same cells and pixels as before, so baselines stay comparable. Neither the palettes, the theme nor the fonts read decoded image pixels, so none of them is part of the image path's build identity, and changing them never moves it.

Not yet drawn from tokens: the type scale's heading steps (headings use the prose size), spacing and corner radii (the layout is on the cell grid). Where the user's theme and font choices are stored, and an in-app picker, come with the settings file.
