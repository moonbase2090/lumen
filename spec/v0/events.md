+++
title = "Events and input"
weight = 60
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Events and input {#events}

## Keyboard {#events.keyboard}

A structured key event carries key, code, modifiers, press/repeat/release and text, building on ideas from the Kitty keyboard protocol. In v0, keystrokes, IME composition and commit, and paste are **not** framed. They go to the shell as ordinary grid input on the PTY, with the same confidentiality as ordinary terminal input and no more.

## Pointer {#events.pointer}

Click, hover and scroll events target a block id and a character offset.

## Block events {#events.block}

Block events are: toggle section, activate link (after the terminal's policy check and confirmation), copy and resize.

## Focus routing {#events.focus}

The focused grid block gets raw VT input. Otherwise input goes to the shell as ordinary grid input.

## Focus-return chord {#events.focus-return}

A reserved focus-return chord is handled by the terminal before any application sees it. Content cannot intercept, remap or draw it, and its hint appears only in chrome. The chord is Ctrl+Shift+Escape on Linux and Cmd+Shift+Escape on macOS. It can be configured only in the user policy file, never by applications or frames.

## No terminal-origin frames on PTY input {#events.no-terminal-frames}

The terminal MUST NOT write Lumen frames to PTY input in v0. Resize is TIOCSWINSZ plus SIGWINCH and nothing else. Focus reporting (mode 1004) stays off, and requests to enable it are ignored. Clicks on Lumen chrome are handled inside the terminal and are sent to no shell. A frame on PTY output that claims terminal origin is treated as a failed frame.

## Paste sanitising {#events.paste}

Bracketed paste is used inside grids. Before the content is wrapped, ESC, C0 controls other than tab and newline, all C1 controls, and the bracketed-paste end marker MUST be removed. A paste that contains a frame introducer is rejected with a notice and is never parsed as a frame.

## Pastejacking {#events.pastejacking}

If the target grid has not enabled bracketed paste (DECSET 2004), a paste containing LF or CR is held. It needs an out-of-band confirmation that shows the full text with control characters visible, and the user may choose to paste it as a single line instead. In non-bracketed paste, any other C0 control (and DEL) is shown as a visible escape in the confirmation and is stripped unless the user keeps it.

## Trust UI out of band {#events.trust-ui}

All permission and trust UI (clipboard, links, capability grants, confirmations) is drawn out of band in terminal chrome, never inside block or grid content.
