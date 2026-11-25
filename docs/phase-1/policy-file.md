# The policy file (Phase 1 terminal)

The terminal reads the user's notification settings from one file. This note says where it is, what it may hold and when it is ignored. The rules come from the [Policy file](../../spec/v0/capabilities.md#caps.policy) section of the capabilities spec and the [Notifications](../../spec/v0/trust.md#trust.notifications) rules.

**Where.** `$HOME/.config/lumen/policy.toml`. Nothing else is read: not the current directory, not the environment, not the command line, not a frame, not a remote host. A program, or a launcher script, cannot turn a setting on.

**What.** One setting per line, `key = true` or `key = false`, with `#` comments and blank lines. The keys are `allow_unsigned`, `os_notifications` and `os_text`; each is off by default.

```
# $HOME/.config/lumen/policy.toml
os_notifications = true
os_text = false
```

**When it is ignored.** The file is ignored, with a notice, and the defaults apply, if it:

- is a symbolic link or not a regular file;
- is not owned by the user running the terminal;
- can be written by group or others;
- sits in a directory that is not owned by the user or that group or others can write to (the directory check is stricter than the spec's file check: anyone who can write the directory can swap the file);
- is larger than 4096 bytes;
- is not valid text, has a line that is not `key = true|false`, names a setting the terminal does not know, or gives a setting twice. A half-read policy is worse than none, so one bad line ignores the whole file.

On a platform without Unix file modes the file is not read yet and the defaults apply.

**Checking it.** `bright --show-policy` prints the file that was read and the settings it gave, the defaults, or why the file was ignored, and exits. A normal run prints the same notice on standard error when the file is ignored.

**The notice in the window.** When the file is ignored the window also draws the notice, as the spec's chrome notice, in the notification area at the top, ahead of every toast. It names the file and why it was ignored, and says the defaults apply. It stays while the defaults apply: a click does not hide it, program output cannot dismiss it, and a flood of toasts from a program cannot push it out. It is never drawn in the trust margin. The settings file works the same way: a settings file that is ignored, or a theme or font file it names that cannot be used, shows its notice in the window, and the notice goes when the file is fixed (the settings file is read again as it changes; the policy file is read once at start, so fixing it takes a restart). Up to four notices are drawn at once. A notice is drawn in full, not cut at three lines as a program's toast is; only a notice taller than the area is cut at its bottom. A window draws the notices once: when a tab is split into panes, they are in the notification area of the pane at the window's top right corner, where a window of one pane has them, and the other panes draw only their own toasts.

**What the settings do today.** They only widen what the notification rules allow. In a real run an unsigned toast and an operating-system notification still need a layer shown to be local, which the terminal cannot show yet, so neither appears; see [the notification rules](../../spec/v0/trust.md#trust.notifications).
