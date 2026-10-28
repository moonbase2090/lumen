+++
title = "Links, clipboard, copy and paste"
weight = 130
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Links, clipboard, copy and paste {#links-clipboard}

## Links {#links}

- **No auto-open.** Links never open automatically.
- **Confirmation.** Activation requires a confirmation drawn by the terminal. It shows the full decoded target (punycode shown, host never truncated), the producer, and a mismatch flag when the visible text looks like a URL that differs from the target.
- **Schemes.** Only `https`, `http` and `mailto` are allowed. Every other scheme is refused. A `file:` link renders as inert text with a "copy path" action.
- **Copy.** Plain copy of a link gives its visible text. "Copy link target" requires the same confirmation. Paste-as-command never takes a link target without that confirmation.
- **Decoding.** A target that does not decode cleanly is refused.

## mailto {#links.mailto}

`mailto` targets are processed as RFC 6068 describes:

1. Split on the literal `?` and `&` first, then percent-decode each part.
2. Refuse the link if the address contains `?`, `&` or a control character. Refuse it if any kept value contains a control character, including CR and LF.
3. Keep only the address and the `to`, `cc` and `subject` fields. Remove `body`, `bcc`, `attach` and everything else.
4. Re-encode the result. The confirmation shows exactly what will be passed to the mail client.

## Clipboard write {#clip.write}

Only a sealed layer that holds the `clipboard-write` grant can write the clipboard, and only after a terminal-drawn confirmation that shows the exact bytes. OSC 52 has no effect. Nothing can read the clipboard.

## Copy {#clip.copy}

- Copy covers only [visible](@/spec/v0/text-runs.md#runs.visible) content and copies code points, not glyphs.
- Command and code text has bidi and invisible characters stripped, as described in [Bidi](@/spec/v0/text-runs.md#runs.bidi), and the count is shown.
- A copy that spans producers shows the split.

## Tables {#clip.tables}

Tables copy as TSV, with quoting as in RFC 4180. Default copy gives the exact code points. Formula escaping (an apostrophe before cells that start with `=`, `+`, `-`, `@`, tab or CR) happens only in the explicit "copy for spreadsheet" action. A formula-risk badge is shown on tables that contain such cells.

## Paste {#clip.paste}

Paste rules are in [Events: paste](@/spec/v0/events.md#events.paste) and [Events: pastejacking](@/spec/v0/events.md#events.pastejacking).
