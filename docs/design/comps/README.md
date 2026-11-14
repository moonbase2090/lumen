# Design comps

What Bright draws today, in each built-in theme. Every image is the terminal's own headless snapshot of real program output: nothing is painted over or mocked up.

| Scene | Light | Dark | High contrast |
|---|---|---|---|
| Grid: plain and coloured output, the 16 ANSI colours, bold, underline and inverse | [grid-light.png](grid-light.png) | [grid-dark.png](grid-dark.png) | [grid-high-contrast.png](grid-high-contrast.png) |
| Rich blocks: two headings, a paragraph, a code block and an image shown by its alt text, each in its unverified-output box | [rich-light.png](rich-light.png) | [rich-dark.png](rich-dark.png) | [rich-high-contrast.png](rich-high-contrast.png) |

Every image also shows the window's own chrome: the trust margin (the gutter on the left and the header strip that reads Bright), and the editor at the bottom.

## How they were made

- **Code:** commit `60e8ec4e183573fad1350c0ddbd2098cdcca66e2`, the design tokens, palettes, theme files, fonts and settings file work, with the header strip renamed to Bright. The comps show those palettes, so they are rendered from that commit, not from `main`.
- **Binary:** `lumen-term`, built in release mode on an Apple silicon Mac at that commit (sha256 `631491f58bc8b1fdb971dd292273f94b54f77c823c5236eb2f161a7d8bb5ac88`).
- **Where:** a fresh macOS 26.6.2 virtual machine, deleted afterwards. The binary and the scene files were copied in and their sha256 checked there before the run.
- **Scenes:** [`scenes.py`](scenes.py) writes the bytes each scene's program prints: text and colour escapes for the grid, and Track 0 frames for the rich blocks.
- **Render:** [`make-comps.sh`](make-comps.sh) runs `lumen-term --size 1200x760 --scale 2 --snapshot` once per scene and theme. Each theme gets a throwaway home directory with only a settings file (`theme = "light"`, `"dark"` or `"high-contrast"`) and a policy file (`images = false`, so the image block shows its alt text).

```sh
python3 scenes.py scenes
./make-comps.sh /path/to/lumen-term scenes out
```

| File | sha256 |
|---|---|
| grid-light.png | `cda6feb1098cab4be022f5a76120bfa075e92f0241f9d9edd9d88b2122c10e22` |
| grid-dark.png | `d1459010a9c7c4b11621929ab2a6a858875425a59d72ec7169aced54696cdf9a` |
| grid-high-contrast.png | `b8eee0910b814ee9756afecbacfae986c137720664dbace29b751673b0ce96ad` |
| rich-light.png | `23e68f55e5eb0678baf3ff04b6fe5220d361bb24df5259a7541d50edba41ba4a` |
| rich-dark.png | `7a1a9d5b373943912cc1291ec435aaa80ee454210391d6d4d44df61f7286141f` |
| rich-high-contrast.png | `ab7724492985210ce07a7589abf038d90e5bf4b2a9757bdcd9e6448b3f817445` |

## What these do not show yet

- **Toasts.** The window draws a toast only for a sealed notification. Unsigned output never draws one there, and sealing is Linux only, so a toast comp needs a Linux machine with the sealed shell.
- **The operating system's title bar.** These are headless snapshots of the terminal's own drawing, not window captures.
- **Headings at their own size.** Headings draw at the paragraph size for now; the design tokens' type scale is not applied to rich blocks yet.
- **Bold.** Bold text draws in the regular weight; there is no bold face yet.
- **Rich blocks after a full screen of output.** The window draws the document from the top and a grid block is as tall as the screen, so the rich-block scene writes its frames first.
- **The image itself.** With images on, the image block shows the decoded image; these comps turn images off to show the alt text.
