# Red-team pass: v0 notifications section

This is a second red-team pass on the Notifications section of `spec/v0/trust.md` (the text on main at the commit named below), written by a reviewer who is not the section's author. It reads the text, then reads the sink model in `crates/lumen-sinks` against it. It runs no attack and describes no working exploit: every finding is a gap in what the text requires or what the model checks.

Reviewed commit: `e276673`. Status: the section should not count toward a gate until R1 to R5 are answered; R6 to R9 are recommended; the model notes (M1 to M5) belong to the sink model, not the spec.

## Scope

- OS notifications only from a sealed 0x22 frame that holds the `notifications` capability, with `notifications.os` false by default and `notifications.os-text` false by default.
- Unsigned, possibly-remote, SSH and multiplexer-pane output only ever a toast, and a toast from unsigned output only with `notifications.allow-unsigned`.
- Interception of OSC 9, 777 and 99, the text rules, the pane label, the rate limits, toast placement and click behaviour.

## What the earlier pass covered, and what it missed

The first pass (on the revisions up to `cd222ad`) found eight gaps, all since resolved. This pass found five more that the same text already had when I last looked: R1, R2, R3, R4 and R5 were there and I did not raise them. R4 is a requirement that an earlier revision stated and a later one dropped.

## Findings

| # | Area | Finding | Suggested fix |
|---|---|---|---|
| R1 | Rate limits | The OS path has only the 1000 ms interval per layer. The 4-per-10-seconds and 16-at-once limits are written for toasts. An OS notification cannot be taken back: it can sit in a history, sync to other devices and survive the terminal. At the interval alone, one layer can send 3600 an hour and N layers N times that, with no global cap. | Give the OS path its own budget: a per-layer count per minute, a global cap across layers, and a fallback. After the budget is spent the layer's notifications are toasts only until the user acts (unmutes, or a quiet period passes). |
| R2 | Text rules | 'Bidirectional controls and zero-width characters removed' is not a testable class. Characters that can still change what the user reads or make a second line: the line and paragraph separators U+2028 and U+2029 (not control characters), the tag characters U+E0000 to U+E007F, the invisible operators U+2061 to U+2064, the Hangul and Mongolian fillers, the soft hyphen, the combining grapheme joiner, variation selectors. Combining marks are not limited, so a short string can stack marks far above and below its line. | State the class by Unicode general category: remove Cc, Cf, Zl and Zp, the tag range and the fillers; limit combining marks to a small number per base character; and count the 60-cell limit in display width, not characters. |
| R3 | Toast placement | Toasts 'never occlude, and are never drawn in, the trust margin', but nothing says toast content is clipped to the toast's own box. Overflowing glyphs (stacked marks, wide characters) are the case the placement rule is for. | Require every toast to be clipped to its box, with no overflow into neighbouring chrome. |
| R4 | Click and OS content | An earlier revision said no program icon, URL, action button or sound is passed to the operating system. The merged text says only 'nothing made clickable' and 'no sound'. Icons, action buttons and reply fields are all offered by the platform notification APIs. | Restore the sentence: no icon, no action button, no reply field and no inline image is passed, and the only action is the one default activation of Click. |
| R5 | Toast eviction | 'Replaces the oldest toast from the layer with the most toasts shown' has no tie-break. It also lets a new toast from layer A displace layer B's toast when B has the most, so a noisy layer can push out a quiet layer's one real notification. | A layer at its own limit evicts its own oldest first. For the global cap, name the tie-break, and prefer evicting the sender's own oldest on a tie. |
| R6 | Pane label | The label is a position (window, tab or pane index) plus a colour and glyph from the layer's opaque id. A toast queued from a layer that has since closed still shows that position, which a different layer may now hold. | Remove a closed layer's queued and shown toasts, and say layer ids are never reused, as pane ids are. |
| R7 | Click token | 'An opaque token the terminal made for that notification' does not say how long it lives. A token from an OS history entry could be activated long after the pane or the terminal is gone. | Tokens are random, single-use, bound to one layer, and invalid after the layer closes or the terminal restarts. A stale token focuses nothing. |
| R8 | Local layer | `allow-unsigned` excludes SSH sessions and possibly-remote layers. SSH is detected from the process tree; shells reached by other means (mosh, a container or cluster exec, a debug bridge, a serial or network console) are not named. | Extend the existing 'when the terminal cannot tell, treat it as one' rule: a layer is local only if its foreground process tree is shown to be local. |
| R9 | User control | 'Muted by the user' is only a condition on the OS path. A toast flood has no stated user control. | Mute covers toasts as well. State a one-step mute and a way to revoke the `notifications` capability, both reachable from the keyboard. |

## Notes on the sink model

These are about `crates/lumen-sinks`, not the spec text.

- **M1.** `cap` counts characters, not cells, so 60 wide characters are 120 cells and combining marks count as cells. This is the R2 width point in code.
- **M2.** `clean` removes `char::is_control`, the U+200B to U+200F, U+202A to U+202E, U+2060, U+2066 to U+2069, U+061C and U+FEFF characters. The characters listed in R2 pass through, including U+2028 and U+2029, so a text can still break to a second line.
- **M3.** `Layer` has `ssh` and `multiplexer_pane` but no possibly-remote flag, though the OS condition names it.
- **M4.** `accepted_times` and `toast_times` only grow. A long run keeps every timestamp.
- **M5.** The per-layer queue of 8, the global cap of 16 and the eviction rule are not modelled, so R5 cannot be tested yet.

## What held up

Interception reads as complete (introducers, terminators, split reads, overflow ending at ESC, CAN or SUB, no reply to the PTY). The OS conditions are evaluated when the notification arrives. The fixed prefix, the pane label and the default of sending no program text to the operating system are the right mitigations for the accepted residual risk, and `notifications.os-text` being a separate key is a good call.

## Status at the re-check

The author answered R1 to R9 in the text. Re-checked at `9b35d4a`:

| # | Status |
|---|---|
| R1 | Answered: an OS budget of 1 per second and 3 per minute per layer and 8 per minute overall, then toasts only until the user acts or ten quiet minutes pass. The overall limit is a per-minute window. |
| R2 | Answered after one correction: the Hangul and other fillers are category Lo, so they are listed by code point, and a combining mark with no base is removed. |
| R3 | Answered: every toast is clipped to its own box. |
| R4 | Answered: no icon, action button, reply field or inline image reaches the operating system. |
| R5 | Answered: a toast never removes another layer's toast. Open as a choice: with four layers each holding four toasts a fifth layer's toast is dropped. |
| R6 | Answered: layer ids are never reused and a closed layer's toasts are removed. |
| R7 | Answered: the token is random, single-use, bound to one layer and invalid after the layer closes or the terminal restarts. |
| R8 | Answered after one correction: 'local' is defined once and both the OS path and `allow-unsigned` use it. |
| R9 | Answered: mute covers toasts, with one-step mute and capability revocation from the keyboard. |

M1 to M5 are about the sink model and stay open there. This is a read of the text, not a test of an implementation.
