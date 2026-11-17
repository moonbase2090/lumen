# The settings file (Phase 1 terminal)

The terminal reads the user's look settings, the theme and the fonts, and the shell to run, from one file. This note says where it is, what it may hold and what happens when part of it cannot be used. The palettes, the theme file format and the floors themes are held to are in the [design system](../design-system.md) note.

**Where.** `$HOME/.config/lumen/settings.toml`, beside the [policy file](policy-file.md). On Windows, where `HOME` is not usually set, the user's profile folder is used instead. It is read when the terminal starts.

**What.** One setting per line, `key = value`, with blank lines and `#` comments on their own line or after a value. Every key is optional.

```toml
# $HOME/.config/lumen/settings.toml
theme = "system"        # system, light, dark, high-contrast, or a theme file
mono_font = "bundled"   # bundled, or a font file
mono_size = 14          # 8 to 72
prose_font = "bundled"
prose_size = 16
shell = "pwsh"          # the program to run when none is named; see below
```

| Key | Values | Default |
|---|---|---|
| `theme` | `"system"` follows the system's light or dark appearance, live; `"light"`, `"dark"` or `"high-contrast"` fixes one built-in palette; a path names a theme file | `"system"` |
| `mono_font` | `"bundled"`, or a path to a font file, for grids, code and the editor | `"bundled"` |
| `mono_size` | a whole number of pixels, 8 to 72 | `14` |
| `prose_font` | `"bundled"`, or a path to a font file, for paragraphs and headings in rich blocks | `"bundled"` |
| `prose_size` | a whole number of pixels, 8 to 72 | `14` |
| `shell` | a program name or path, in quotes: what the window runs when no program is named on the command line | not set |

A path is a value with a `/` in it: absolute (`"/Library/Fonts/Mine.otf"`), under the home directory (`"~/themes/mine.toml"`) or relative to the settings file's directory (`"./mine.toml"`, `"themes/mine.toml"`).

**The shell.** When no program is named on the command line, the window runs, in this order: the `shell` setting; the `SHELL` environment variable; then, on Windows, `pwsh.exe` if it is on the path, `powershell.exe` if it is, the program `COMSPEC` names, and `cmd.exe`; on every other system, `/bin/sh`. A value that is empty counts as not set, and a program the user names is started as given: if it does not exist, starting it fails with the system's message. A program named on the command line always wins. `bright --show-settings` shows the `shell` setting (`default` when the file does not set it).

**When it is ignored.** The whole file is ignored, with a notice, and the defaults apply, if it is not a regular file, is larger than 4096 bytes, is not UTF-8 text, or has a line that is not a known key written once with a good value. A half-read file is worse than none.

**When part of it is left out.** A theme file that is refused (see the [design system](../design-system.md#theme-files) note), or a font file that cannot be read, is not a font with Latin letters, or is over 32 MiB, is left out with a notice. Its default stands in for it alone, and the rest of the settings still apply.

**Not policy.** The look settings change how things look and never what a program may do, and the shell is the user's own choice of what to run, not something a program can set, so unlike the policy file they have no owner or mode checks. A theme cannot make text unreadable: a theme whose text falls under the spec's 3:1 visibility floor is refused.

**Checking it.** `bright --show-settings` prints the file that was read, the settings that apply and a notice for anything left out, and exits. A normal run prints the same notices on standard error.

**What is not here yet.** Changes to the file take effect at the next start; a change to the system's appearance takes effect at once. There is no in-app picker yet.
