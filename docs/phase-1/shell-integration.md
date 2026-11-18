# Shell integration (Phase 1 terminal)

The terminal never picks a shell for the user. It runs the one it is told to: a program given on the command line, else the `shell` setting, else `$SHELL`. This note says what it adds to a shell it starts, how, and how to turn that off.

**What it adds.** When the shell is zsh, bash, fish or PowerShell and it is started directly (no arguments), the terminal starts it with a small hook script that writes the standard prompt marks:

- OSC 133 `A` at the start of the prompt and `B` at its end, where the command is typed;
- OSC 133 `C` when a command starts running and `D;status` when it ends, with its exit status;
- OSC 7 with the working directory, as `file://host/path` with the path percent-encoded.

A shell with no hook, or any program started with arguments such as `-c`, runs exactly as it would anywhere else.

**How, with none of the user's files changed.**

| Shell | How the hook is loaded |
|---|---|
| zsh | `ZDOTDIR` points at a directory of the terminal's own. Its `.zshenv` puts the user's `ZDOTDIR` back, reads the user's `.zshenv`, then adds the hooks with `add-zsh-hook`. Every later file zsh reads is the user's own. |
| bash | `--rcfile` names a file that reads the user's `~/.bashrc`, then adds the hooks (`PROMPT_COMMAND`, `PS0` or a `DEBUG` trap on bash older than 4.4, and a `PS1` end mark). |
| fish | the terminal's `share` directory goes first on `XDG_DATA_DIRS`, so fish reads the hook from `vendor_conf.d` before the user's `config.fish`. fish 4 and later write the marks themselves, so the hook does nothing there. |
| PowerShell | `-NoExit -Command` dot-sources the hook after the user's profile has loaded. It wraps `prompt` and the Enter key. |

The hook files are written to a private directory (mode `0700`) that is removed when the session ends. The shell's environment also has `BRIGHT_SHELL_INTEGRATION=1`, which a user's own rc file can test.

**Turning it off.** `shell_integration = false` in the [settings file](settings-file.md). On by default.

**The sealed layer.** On Linux a listed zsh started directly can have a sealed layer, which has a hook of its own that sends sealed frames. That shell does not also get these marks. A shell that is not sealed gets them.

**What the terminal does with them.** The grid parser still removes OSC 133 and OSC 7 before the emulator sees them, so nothing in the window changes. It now notes each one (the last 256, oldest first) and the session's document gives them out as `Document::marks()`, for what reads them next and for tests.

**What does not happen yet.** PowerShell's line editor asks the terminal where the cursor is (`ESC [ 6 n`), and the terminal does not answer that on Linux or macOS yet, so PowerShell is not usable there; on Windows the pseudo-console handles it. The marks do not depend on it.
