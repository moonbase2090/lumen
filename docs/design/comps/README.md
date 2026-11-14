# Design comps

What Bright draws today, in each built-in theme. Every image is the terminal's own headless snapshot of real program output: nothing is painted over or mocked up.

| Scene | Light | Dark | High contrast |
|---|---|---|---|
| Grid: plain and coloured output, the 16 ANSI colours, bold, underline and inverse | [grid-light.png](grid-light.png) | [grid-dark.png](grid-dark.png) | [grid-high-contrast.png](grid-high-contrast.png) |
| Rich blocks: two headings, a paragraph, a code block and an image shown by its alt text, each in its unverified-output box | [rich-light.png](rich-light.png) | [rich-dark.png](rich-dark.png) | [rich-high-contrast.png](rich-high-contrast.png) |

Every image also shows the window's own chrome: the trust margin (the gutter on the left and the header strip that reads Bright, in the terminal's background colour and set off by a one-pixel divider), and the editor at the bottom.

## How they were made

- **Code:** commit `894bcc98caa5b44b556f6429a631ffbbdc5eae9a`, the design tokens, palettes, theme files, fonts and settings file work, with the header strip renamed to Bright and the trust margin drawn in the terminal's background colour with a one-pixel divider. The comps show those palettes, so they are rendered from that commit, not from `main`.
- **Binary:** `lumen-term`, built in release mode on an Apple silicon Mac at that commit with `cargo build --release --locked -p lumen-term` (sha256 `074b806e0f1eb28c9277c19cbbd91ebb87cd1f93465f1722f132fba930d876ed`).
- **Where:** a fresh macOS 26.6.2 virtual machine, deleted afterwards. The binary and the scene files were copied in and their sha256 checked there before the run.
- **Scenes:** [`scenes.py`](scenes.py) writes the bytes each scene's program prints: text and colour escapes for the grid, and Track 0 frames for the rich blocks.
- **Render:** [`make-comps.sh`](make-comps.sh) runs `lumen-term --size 1200x760 --scale 2 --snapshot` once per scene and theme. Each theme gets a throwaway home directory with only a settings file (`theme = "light"`, `"dark"` or `"high-contrast"`) and a policy file (`images = false`, so the image block shows its alt text).

```sh
python3 scenes.py scenes
./make-comps.sh /path/to/lumen-term scenes out
```

| File | sha256 |
|---|---|
| grid-light.png | `ba4a1b279fdde9b0f64f840281c2d61cf3ca523d475abdf555a28531c2ceb93a` |
| grid-dark.png | `ba80748c7e8ff23a699f170baf73efa05e943bcc9595c21d8817f1ed7835a721` |
| grid-high-contrast.png | `4f2e6151dd14ed5996bfc28b3723ddd4bd05b2a6b8f6f4bd3538439232175b8f` |
| rich-light.png | `3fab625eab1913cc26c93eca2c55d396f473ea39b058a52fc1f60ef3f14e5974` |
| rich-dark.png | `fbd68bf93c6483cd9576a5e409eacaa970b26f62850b01b3118a46c925308eb7` |
| rich-high-contrast.png | `6748902bfbc6d1dc0a98b9689a7cecad43224f9a178edb5ffd1589cc01ee32b5` |

## What these do not show yet

- **Toasts.** The window draws a toast only for a sealed notification. Unsigned output never draws one there, and sealing is Linux only, so a toast comp needs a Linux machine with the sealed shell.
- **The operating system's title bar.** These are headless snapshots of the terminal's own drawing, not window captures.
- **Headings at their own size.** Headings draw at the paragraph size for now; the design tokens' type scale is not applied to rich blocks yet.
- **Bold.** Bold text draws in the regular weight; there is no bold face yet.
- **Rich blocks after a full screen of output.** The window draws the document from the top and a grid block is as tall as the screen, so the rich-block scene writes its frames first.
- **The image itself.** With images on, the image block shows the decoded image; these comps turn images off to show the alt text.
