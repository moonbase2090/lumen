# Links, copy and paste safety

What `lumen-term` has for the rules in the specification's [links, clipboard, copy and paste](../../spec/v0/links-and-clipboard.md) page. It is behind the `links-clipboard` Cargo feature of `lumen-term`, off by default, and nothing in the terminal calls it yet: there is no link frame in the v0 frame table, no selection and no paste handler in the window, so these are the checks those pieces will call.

## Links (`link`)

`check(target, visible)` says what a link target may be:

- **Allowlist.** `https`, `http` and `mailto` need a confirmation the terminal draws. A `file` target is inert text with its decoded path offered to copy. Every other scheme, and a target with no scheme, is refused.
- **What the confirmation shows is what is used.** For a web target the host is lower case and shown in ASCII, each non-ASCII label as punycode, and is never cut; the result says when the name is internationalised. A target longer than 2048 bytes, or a host longer than 253, is refused rather than cut.
- **Refused as not decoding cleanly:** a malformed percent escape, bytes that are not UTF-8, a control character (raw or decoded), a space, and a default-ignorable or bidirectional code point (raw or decoded), which could hide part of the target.
- **A user name in a web target is refused.** The specification does not say this; it is a choice, because the part before the `@` is what a lookalike address hides behind.
- **Mismatch flag.** Set when the visible text looks like a URL (a web scheme, a leading `www.`, or a dotted name with an alphabetic last label) and differs from the target after normalisation. A downgrade from `https` to `http` is a difference.
- **mailto** follows RFC 6068 as the specification states: split on the literal `?` and `&` first, then decode each part; refuse an address containing `?`, `&` or a control character, and refuse a kept value containing a control character, CR and LF included; keep only the address, `to`, `cc` and `subject`; re-encode, and show exactly that.
- **Copy.** `copy_plain` gives the visible text. `copy_target` gives the target only when the caller says the confirmation was shown and accepted, and the same call serves a paste-as-command.

## Copy (`copy`)

`copy(runs)` returns the logical code points of the visible runs. Hidden runs are left out and counted. In code text the bidirectional controls are stripped; in command text those and the zero-width and invisible characters and tag characters are stripped; paragraph and heading text is untouched. The number stripped is returned, and a copy that spans producers returns its parts so the terminal can show the split.

## Paste (`paste`)

`judge(text, bracketed)` removes the bracketed-paste end marker, ESC, C0 controls other than tab and newline, DEL and the C1 controls; rejects a paste that contains a frame introducer (any `ESC _`); and strips bidirectional and invisible characters from pasted command text, counting them. Without bracketed paste, a paste with a line break or another control is held: the result carries a preview with every control shown as a visible escape, the paste as a single line, and the paste as written (still without ESC and C1).

## Not done

No tables copy (TSV and the spreadsheet action), no link frame or link drawing, no confirmation drawing, no selection, no wiring into the window or a grid's paste, and no check of whether bracketed paste is on (the caller says). The visibility test that decides a run's `visible` flag is the caller's.
