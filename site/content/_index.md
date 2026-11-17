+++
title = "Lumen"
template = "index.html"
+++

<section class="section">

## Not backwards compatible, by design

Lumen is a new protocol rather than another escape-code extension. Grid blocks give existing terminal apps a native place inside a Lumen document, and unverified output is always shown as plain, inert text.

</section>
<section class="section">

## Status

`lumen-term` has a native window backend behind the off-by-default `window` feature. It is implemented and tested on Linux, macOS and Windows, and a real native window was captured on each: Linux under X11 on a virtual display, macOS in a clean virtual machine, and Windows on a dedicated test profile. None of these was a physical display.

The current public draft is **Lumen Protocol v0, revision 25 (frozen snapshot)**. It is not stable, and breaking changes are expected. [Read the spec](@/spec/v0/_index.md). The decisions behind it and the risks accepted are on the [decisions page](@/decisions.md).

**Phases, not promises. No dates are set.**

<ol class="phase-timeline" aria-label="Lumen phases">
<li class="phase phase-done">
<div class="phase-body">
<div class="phase-heading"><span class="phase-state">Done</span><h3>Phase 0.5</h3></div>
<p>The decision step is finished. The Lumen maintainers adopted Arm B for v0: Track 0 rich everywhere, with the sealed upgrade, and multiplexer panes keep the sealed relay. The evidence and the unmeasured work are in <a href="https://github.com/moonbase2090/lumen/blob/main/docs/phase-0.5/decision.md">the decision record</a>.</p>
</div>
</li>
<li class="phase phase-current" aria-current="step">
<div class="phase-body">
<div class="phase-heading"><span class="phase-state">Current</span><h3>Phase 1</h3></div>
<p>Lumen is in Phase 1 on Arm B, the chosen v0 design. Work continues in the terminal, decoder, image path, resource model, and sealed shell. The <a href="https://github.com/moonbase2090/lumen#status">README status</a> lists what works and what remains.</p>
</div>
</li>
<li class="phase phase-later">
<div class="phase-body">
<div class="phase-heading"><span class="phase-state">Later</span><h3>After v0</h3></div>
<p><a href="/spec/v0/multiplexers/#mux.tmux-screen">tmux and GNU screen support</a> remains post-v0 work.</p>
</div>
</li>
</ol>

### Phase 1 progress

Phase 1 is current on Arm B. The terminal runs programs on the upstream VT core and draws the trust margin itself. A sandboxed image decoder and an image manager enforce the image rules. Both are connected to the terminal: its image host calls the image manager and decodes out of process. Images are on by default, on Linux and macOS (Windows is unsupported for images in v0 and shows the alt text).

The unlock rule still applies: a build shows an image only when gate records exist for both of its build identities. The shipped records do, so the terminal shows the image, and the setting `images = false` always shows the alt text. [Lumen Term's release criteria](https://github.com/moonbase2090/lumen/blob/main/docs/release-criteria.md) set the decoder fuzzing gate. Each image format must reach at least 250 CPU-hours on each platform for both the decoder build identity and the terminal image-path build identity, with no open crash or confirmed hang. The Lumen maintainers adopted this rule in place of an earlier coverage-plateau stop rule that the campaign never reached. An open crash fails that build identity. Fuzzing continues after release.

Status: image testing is complete. The gate records for the current decoder and image-path identities are committed, and images are on by default on Linux and macOS. The guarded fuzz runs (PNG 352, JPEG 378 and WebP 412 CPU-hours, no open crash or confirmed hang) were on Linux and on earlier builds: the decoder's sandbox and manifest changed afterwards, and the frame parser and image host were not fuzzed.

The JPEG run's 930 timeouts were replayed again under the production limits and none was a hang. The WebP guard, which checks prefix codes before the library decodes them, is shipped. The JPEG decoder is pinned to zune-jpeg 0.5.16-rc3, a release candidate: 0.5.15, the last stable release, panics on 16 malformed JPEGs found by fuzzing, and rc3 answers all of them without a panic. Those inputs are now regression tests. Because this changes the decoder build identity, the earlier JPEG fuzzing covers only the earlier identity.

The window has actions of its own, from a menu item or a shortcut: copy the selected text, paste the clipboard text, select all, larger and smaller text and back to the display's size, close and quit. On macOS they are the usual Command chords (Command+C, Command+V, Command+A, +, -, 0, W and Q) with a menu bar that also carries the system's hide, hide others, show all, minimise and zoom items. On Linux there is no menu, and the chords are Control+Shift+C, Control+Shift+V, Control+Shift+A and Control+Shift with +, - and 0, so no key a program uses is taken. Paste goes through the paste rules: when the program has turned on bracketed paste the text goes in between the paste markers, line breaks kept, with the escape and control characters and a pasted end marker removed; otherwise text with a line break or a control goes in as one line, because the terminal cannot yet ask first. Text carrying a frame introducer is refused either way. Dragging the pointer over the text selects it, in the grid and across the wrapped lines of a rich block; a copy of wrapped text comes back as one line, with the blank the wrap dropped kept as a space, and a program that asked for mouse reports gets the pointer unless Shift is held. Selecting by keyboard, by word or by line, and selecting inside a strip are not there yet.

A grid parser drops string escapes. The notification path in the window draws sealed notifications as toasts, and a click dismisses a drawn toast. An operating-system backend is implemented and tested on Linux, but the window does not yet show a notification from a real program. Notification settings read only from the user's policy file.

The window works with the system's input methods, dead keys and compose sequences. Text an input method commits reaches the program once, as typed text, with control characters removed so it cannot carry an escape sequence; a dead key or compose sequence arrives as the letter it makes. While a composition is unfinished the program sees nothing, and losing focus or turning the input method off drops it. On macOS, Option with a key that makes an accented or other non-ASCII letter sends that letter, not an Escape prefix. The text being composed is not drawn in the window yet, so it shows only in the system's candidate window, which sits at the editor row and follows the window as it is resized or zoomed.

The window shows the title a program sets with OSC 0 or OSC 2 in its own title bar, never in the page, followed by the terminal's name, as in `build log - Bright`. An empty or blank title, or none at all, shows the name alone. A title with a control character is refused whole, bidirectional controls are removed, and a long title is cut so the name stays last.

The window follows the display it is on: when it moves to a display with a different scale factor, text is drawn again at that display's own scale (rounded to a whole number, up to 4), so it stays sharp, the program is told its new window size, and a zoom you made stays the same number of steps from the display's scale. A scale set with the `--scale` option is kept as set. A fractional factor such as 1.5 is drawn at the nearest whole scale.

On Linux, a listed zsh that the terminal starts directly can have a sealed layer. The terminal creates the seal channel first, starts the shell with a shim and that channel, and starts the holder and delivers a key only when the sealing conditions say to seal. The holder then seals a command start, a prompt boundary or a command end when that shell asks, and refuses every other request, including from a program the shell starts. If the decision is not to seal, or the holder cannot start, the shell still runs and the layer stays grid-only, with no holder and no key. macOS and other systems stay grid-only.

In a rich block, paragraph, heading and image alt text is now drawn in a proportional face (IBM Plex Sans) laid out by each glyph's own width and wrapped at the block's pixel width, instead of on the monospace grid; code blocks, grids, toasts and the trust margin stay on the grid. A character the proportional face lacks, or one that takes two cells, is drawn from the monospace face. Tests run on Linux and macOS check the glyph widths, and a real-window capture from a clean macOS guest is in the repository.

Paragraph and heading text keeps zero-width characters (ZWJ, ZWNJ and variation selectors), ZWSP and WJ act only as line-break controls, and bidirectional controls are closed at every line and block end. These rules are now on by default; a build without default features keeps the earlier rules. Where the terminal composes text a person relies on to judge trust (notifications, link labels and targets, the code-block language label), invisible and bidi code points show as visible escapes such as ⟨U+200B⟩. When typed input cannot be sent because the program is not reading it (or has ended), the window says so with a notice the terminal draws in the notification area, never in the trust margin, and counts what was dropped.

The terminal's colours come from a design system with light, dark and high-contrast palettes, each checked against its own contrast floors. The window follows the system's light or dark appearance by default and switches live, with no restart. A settings file read at start chooses a fixed palette or a theme file (a built-in palette with colours changed, refused whole if any text would fall under the 3:1 visibility floor), and the face and size of monospace text and of prose. A file that cannot be used is left out with a notice and its default applies. There is no in-app picker yet.

The window draws the cursor of the program's live screen as the program set it: block, underline or bar, blinking or steady, shown or hidden. A blinking cursor restarts its cycle on a key, pasted or composed text, or output, and an unfocused window draws it dimmed and steady, with a block as an outline.

A budget model, `lumen-budget`, holds the numbers of the resource-limits table (the pooled document budgets with the local reserve and the producer and unsigned shares, the per-second rates and the limits on a single item, including the largest legal image of 16 megapixels) with language-neutral test vectors run against it; nothing in the terminal calls it yet, so these limits are not newly enforced. Nothing here is released. The progress is listed in the [README](https://github.com/moonbase2090/lumen#status).

</section>
