# tmux and screen support: design plan (post-v0)

A plan, not an implementation. Nothing in the terminal, the producers or the wire format changes with this document. It says how Lumen output could reach the terminal through tmux and GNU screen after v0, what breaks, and how each step would be tested, so the maintainers can decide whether and in what order to build it.

## Today

In v0, if `TMUX` or `STY` is in a layer's environment, Track 0 on that layer renders as escaped text ([Multiplexers](../../spec/v0/multiplexers.md#mux.tmux-screen)). Two other rules also apply: unsealed Track 0 on the alternate screen renders as escaped text ([alternate screen](../../spec/v0/multiplexers.md#mux.alt-screen)), and every unsealed frame is treated as possibly remote ([trust](../../spec/v0/trust.md#trust.possibly-remote)). The only multiplexer that can show rich panes is a Lumen-aware one on a sealed layer, which is a different project from supporting the multiplexers people already run.

## What any design must keep

- No new wire format. A frame is the same Track 0 frame, whole, with its 4 KiB string caps.
- Unsealed only. Keys are never given to a multiplexer server ([Keys](../../spec/v0/multiplexers.md#mux.keys)), so frames that arrive through tmux or screen are unsealed Track 0 and are possibly remote. A sealed-looking frame that arrives this way is refused.
- A multiplexer pane is never a local layer. Nothing drawn from it may look verified, and nothing may draw over another pane or over the trust margin.
- Images stay behind the platform unlock. A multiplexer path adds no way around it.
- Features ship on by default once they pass their tests, with a setting to turn them off.

## Approach: passthrough, producer-wrapped, terminal-anchored

Both multiplexers can pass a device control string to the outer terminal without interpreting it. The producer (a program running in the pane) wraps each frame; the multiplexer unwraps it and writes the inner bytes to the terminal it is attached to; the terminal sees an ordinary frame.

### tmux

- tmux 3.3 and later have the `allow-passthrough` option, off by default, with values `on`, `off` and `all`. The wrapper is `ESC P tmux ; <payload> ESC \`, with every `ESC` in the payload doubled. With `on`, tmux passes the payload through only while the pane is visible; `all` passes it for invisible panes too. The plan uses `on` and never asks for `all`.
- The payload does not enter the pane's own screen model. tmux does not store it, and it repaints the pane from its own model afterwards.
- Nested tmux (tmux inside tmux, or tmux over SSH inside a local tmux) needs the wrapper applied once per level, with the escape doubling applied again at each level.

### GNU screen

- screen passes a device control string through to the display, but with a limit on how long one string may be. The limit has to be measured on current releases before anything is promised. Our frames are length-prefixed strings up to the 4 KiB cap, and image data is chunked, but the start frame and any paragraph frame cannot be split, so a small limit rules out parts of the frame set.
- screen is therefore a second step, taken only if the measurement shows the frame set fits. Otherwise screen stays at escaped text, and that is documented.

### Who does what

- **The producer** decides whether to wrap. It already knows it is inside a multiplexer (`TMUX` or `STY`). For tmux it asks the server for `allow-passthrough` and wraps only if the answer is `on` or `all`; if passthrough is off, tmux would drop the frames silently and the reader would see nothing, so it falls back to plain text. This logic belongs in the producers' shared helper, and the specification gets a short MAY for it.
- **The terminal** does not need to know the multiplexer's pane layout. It treats the arriving frame like any unsealed frame, anchored at its own cursor when the frame arrives, drawn in a labelled, clipped container marked as coming through a multiplexer (as the [threat model](../../spec/v0/threat-model.md) already describes), and still possibly remote.
- **The user** gets one setting to turn the path off, which restores today's escaped text exactly.

## What breaks, and the answer for each

| Problem | Why | Plan |
| --- | --- | --- |
| The alternate screen rule | tmux and screen run on the outer terminal's alternate screen, so today's rule would turn every frame into escaped text. | The alternate-screen rule is relaxed only for frames that arrive wrapped by a multiplexer the layer is known to be running, and stays for a program that enters the alternate screen itself. This is a specification change and needs a maintainer decision. |
| Content is not kept by the multiplexer | tmux repaints the pane from its own model, so a drawn block is not redrawn after a scroll, a pane switch, copy mode, a resize, or a detach and reattach. | The block is anchored to the terminal's own cells. When the multiplexer writes over those cells (which is what its repaint does), the block collapses to its alt text, as an image is cleared when its cells are overwritten. The result is transient blocks. This is stated as the behaviour, not hidden. |
| Interleaving | The multiplexer writes its own output (status line, repaints) between passthrough writes, so a multi-frame image may be split by unrelated bytes. | The detector already ignores bytes outside frames, and image reassembly is in order by offset. Whether other bytes can fall between two frames of one image has to be measured. If they can, the terminal tolerates it, and if reassembly breaks, the image shows alt text. |
| Size limits | A multiplexer may cap one passthrough string. | Measured per multiplexer and version. The producer chunks image data to fit the limit it finds, never below the frame's own minimum. |
| Several clients | tmux can have more than one client attached to a session. | Measured. If tmux sends a frame to every attached client, each client's terminal draws it independently, which is acceptable; if it sends it to a stale one, the block is simply not drawn there. |
| Forgery | Any program in the pane, including a remote host reached over SSH in that pane, can write the wrapper when passthrough is on. | That is the possibly-remote model: it can draw an inert, labelled block in its own pane's area and nothing more. Sealed types arriving this way are refused, counted, and never accepted. |
| Passthrough off or unknown | tmux drops the frames, the reader sees nothing. | The producer checks the option first and falls back to text; a pane written to by a program that does not check shows nothing, which is the same as today for a program that does not know Lumen. |

## Test plan

Each step has a test that fails if the claim is false. All of it runs headless on Linux and macOS in CI where the multiplexer is installable; the Windows console is out of scope (images are unsupported there in v0).

1. **Byte round trip.** A script writes real frames, wrapped and with escapes doubled, inside a real tmux (the oldest supported release, 3.3, and the current one) on a pty, and the outer side of that pty is captured. Assert the captured bytes contain the frames exactly, with `allow-passthrough` on; none with it off; none from an invisible pane with `on` and the frames with `all`. Repeat for nested tmux. Record the longest passthrough string that arrives intact.
2. **Same for screen.** Record the longest string that arrives intact and which of the v0 frame types fit.
3. **Interleaving.** With the multiplexer redrawing (a status line that updates, a second pane that prints) while a multi-frame image is sent, count how often foreign bytes fall between two frames of the same image. Feed the captured streams to the detector and the image reassembly and assert the image is drawn whole or shown as alt text, never garbled and never crashing.
4. **Terminal behaviour.** Under `lumen-term --snapshot`, with tmux as the program: the image appears once; it collapses when the multiplexer repaints those cells; a scroll, a pane switch and a detach and reattach each leave alt text or nothing and never a stale image; the container is labelled and clipped; the trust margin is untouched.
5. **Alternate screen.** The relaxed rule applies to a multiplexer's frames and not to a program that enters the alternate screen on its own inside the pane; both cases are tested with the same bytes.
6. **Forgery.** Sealed-looking frames, frames with a bad header, oversized strings and frames that claim a local source, all arriving wrapped, are refused or labelled possibly remote, and counted. The existing forged-passthrough cases in `lumen-evidence` are extended to cover the wrapper.
7. **Off switch and unknown versions.** With the setting off, the output is byte-identical to today's escaped text. With a tmux older than 3.3, or screen with a short limit, the producer falls back to text and the terminal behaves as today.
8. **Property test.** Random wrapper nesting depth, random escape placement and random splits of the byte stream at every position never make the terminal accept a frame it would not accept unwrapped.

## Order of work

1. Measurements only: steps 1 to 3, as a small tool and a report, no change to the terminal. This answers the open questions below and may end the plan for screen.
2. Specification change (a new revision): the multiplexer rule, the alternate-screen relaxation, the labelled container, the setting and the producer MAY. Needs a maintainer decision and a review as a change to the specification.
3. Terminal: the relaxed rule, anchoring and collapsing, the setting.
4. Producers' helper: detection, the option check, wrapping, chunking to the measured limit.
5. Tests 4 to 8 in CI, then the documentation: the multiplexers page, the platform page and the README.

## Open questions for the maintainers

- May the alternate-screen rule be relaxed for a known multiplexer, as above, or should the multiplexer path be limited to a program that is not on the alternate screen at all (which would exclude the common case)?
- Are transient blocks, which disappear when the multiplexer repaints, acceptable for the first version, or is a block that survives a repaint required? The second needs the multiplexer to cooperate (a Lumen-aware multiplexer) and is the sealed multiplexer project, not this one.
- Is screen worth doing if its string limit is small, or should it stay at escaped text and be documented?
- Which tmux releases are supported: 3.3 and later is the earliest that has `allow-passthrough`.
