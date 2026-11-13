# The settings file (Phase 1 terminal)

The terminal reads the user's look settings, the theme and the fonts, from one file. This note says where it is, what it may hold and what happens when part of it cannot be used. The palettes, the theme file format and the floors themes are held to are in the [design system](../design-system.md) note.

**Where.** `$HOME/.config/lumen/settings.toml`, beside the [policy file](policy-file.md). It is read when the terminal starts.

**What.** One setting per line, `key = value`, with `#` comments and blank lines. Every key is optional.

```toml
# $HOME/.config/lumen/settings.toml
theme = "system"        # system, light, dark, high-contrast, or a theme file
mono_font = "bundled"   # bundled, or a font file
mono_size = 14          # 8 to 72
prose_font = "bundled"
prose_size = 16
```

| Key | Values | Default |
|---|---|---|
| `theme` | `"system"` follows the system's light or dark appearance, live; `"light"`, `"dark"` or `"high-contrast"` fixes one built-in palette; a path names a theme file | `"system"` |
| `mono_font` | `"bundled"`, or a path to a font file, for grids, code and the editor | `"bundled"` |
| `mono_size` | a whole number of pixels, 8 to 72 | `14` |
| `prose_font` | `"bundled"`, or a path to a font file, for paragraphs and headings in rich blocks | `"bundled"` |
| `prose_size` | a whole number of pixels, 8 to 72 | `14` |

A path is a value with a `/` in it: absolute (`"/Library/Fonts/Mine.otf"`), under the home directory (`"~/themes/mine.toml"`) or relative to the settings file's directory (`"./mine.toml"`, `"themes/mine.toml"`).

**When it is ignored.** The whole file is ignored, with a notice, and the defaults apply, if it is not a regular file, is larger than 4096 bytes, is not UTF-8 text, or has a line that is not a known key written once with a good value. A half-read file is worse than none.

**When part of it is left out.** A theme file that is refused (see the [design system](../design-system.md#theme-files) note), or a font file that cannot be read, is not a font with Latin letters, or is over 32 MiB, is left out with a notice. Its default stands in for it alone, and the rest of the settings still apply.

**Not policy.** The settings change how things look and never what a program may do, so unlike the policy file they have no owner or mode checks. A theme cannot make text unreadable: a theme whose text falls under the spec's 3:1 visibility floor is refused.

**Checking it.** `bright --show-settings` prints the file that was read, the settings that apply and a notice for anything left out, and exits. A normal run prints the same notices on standard error.

**What is not here yet.** Changes to the file take effect at the next start; a change to the system's appearance takes effect at once. There is no in-app picker yet.
