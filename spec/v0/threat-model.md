+++
title = "Threat model and accepted residual risks"
weight = 160
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Threat model and accepted residual risks {#threat}

## Default scope {#threat.scope}

**In scope:**

- Untrusted output, from local or remote programs, including a same-uid process that only writes bytes.
- Spoofing of commands, producers and trust UI.
- Resource exhaustion.
- Parser and decoder exploits.
- Copy and paste deception.

**Out of scope by default:** a process that runs code as the user to attack the terminal, the shell or a key holder (for example by debugging, memory reads or injection). That adversary is in scope only in [hardened mode](@/spec/v0/hardened-mode.md). Also out of scope: root, `CAP_SYS_PTRACE`, and remote root or the local SSH client for anything they can observe.

## Parity rule {#threat.parity}

A finding that needs same-uid code execution is a defect only if it makes Lumen worse than a conventional terminal. An example would be adding a secret, socket or capability that same-uid code could not otherwise get. No Lumen asset may give more than the PTY-write path or the user's own dotfiles already give.

## Threats and mitigations {#threat.table}

| Threat | Mitigation |
|---|---|
| Escape or frame injection from output | Sealed frames only under held per-layer keys; one pipeline; failed frames render inert; grid parser isolation; introducer fuzzing |
| Spoofed commands or trust UI | Command structure only from sealed shell integration; unverified output containers; trust margin painted only by the terminal; OSC title never in trust UI; reserved focus-return chord |
| Key exposure | Root key never leaves the terminal; per-layer holders; delivery only over an inherited fd to a listed shell's integration; never in the environment, argv or PTY; non-dumpable, mlocked key pages |
| Replay | AEAD nonce of direction and sequence number; strictly increasing per-layer sequence numbers; no mid-session rekey |
| Forged frame length or framing desync | Header authenticated before the length is used; bounded abort and resume; byte-rate floor; rate pin with user-only restore |
| Copy and paste deception | Visible-only, code-point copy; revision-checked selections; bidi and invisible stripping; confusable flags; paste sanitising and confirmation |
| Malicious images | Sandboxed decoder, input and output limits, reply validation, budgets reserved before allocation, release gates |
| Malicious links | No auto-open; confirmation with the full decoded target; scheme allowlist; mailto filtering |
| Clipboard | Writes only from granted sealed layers with an exact-bytes confirmation; OSC 52 inert; no reads |
| Policy tampering | Owner and mode checks; never read from the current directory, environment, frames or remote hosts |
| Resource exhaustion | Per-item limits, aggregate budgets, a reserved local share and per-producer caps |
| Terminal reply abuse | Fixed-content reply allowlist, rate limits, no terminal-origin frames on the PTY |
| Side-channel peers | Per-user 0700 directory, one-time token, holder credential checks, staged content committed only by an authenticated PTY marker |
| Remote hosts | No keys over SSH in v0; unsealed output is treated as possibly remote |
| Multiplexers | Text by default; a Lumen-aware multiplexer only through the sealed multiplexer layer |

## Accepted residual risks {#threat.residuals}

The maintainers accepted the following residual effects for v0. They are described only by their effects. Each one is rated Low or Informational, and none gives unverified output any authority.

**Output and display**

- **Theme fingerprinting.** Colour-query replies expose the user's palette, and changes between light and dark themes, to any program that can query the terminal, including programs on remote hosts. This is a privacy effect, not an integrity effect.
- **Reply bytes in line editors.** The fixed bytes of allowed replies and reports can land in a line editor's input after the querying program exits. No CR or LF is ever sent, so nothing runs without the user's action.
- **Reply-format edge cases.** Unusual query forms get no reply or a fixed reply. Some programs that probe the terminal time out and fall back. This is a compatibility cost.
- **Fake prompts after an unverified boundary.** A foreground program can cause an extra unverified boundary and then print text that looks like a prompt. The boundary stays labelled unverified with no exit status. Printing a lookalike prompt is possible in every terminal.
- **Alternate-screen and reflow placement.** Track 0 placement across alternate-screen changes, resize and reflow is only loosely specified. The content stays clipped inside its container.
- **Multiplexer passthrough.** With non-default tmux passthrough settings, Track 0 may render in a labelled, clipped container instead of as text. It cannot draw over panes.
- **Local-treatment opt-in.** A user who sets `track0.treat_unsealed_as_local` on a profile gets local treatment for remote output launched from that profile. A persistent warning is shown.
- **Signal ambiguity.** `LUMEN=0` means Track 0 only, but some tools may read it as "disabled". The only effect is that such a tool emits no Track 0.

**Availability of sealing**

- **Corrupted frames.** Sealed frames that arrive damaged are rejected, and the layer can fall back to grid mode until the user restores it.
- **Slow shell start.** A shell with heavy startup files may miss the ready window. It then runs as grid plus Track 0, with a notice.
- **Sandboxed and user installs.** Sandboxed terminals get no shell integration. In hardened mode, user-space installs get none either, and the root-owned binaries cannot be inspected by the user.

**Local timing and same-user processes**

- Local timing issues, and other processes running as the same user on the user's own machine, are out of scope in default mode, as with existing terminals. Hardened mode narrows them.

**Image path**

- System graphics drivers that the image path uses at run time without linking them are outside the build identity, so a driver update does not re-run the decoder gates. Drivers are at parity with every GPU application, and uploads are size-checked first.

## Independent review {#threat.review}

The specification went through repeated, independent adversarial security reviews. Each review was graded with a fixed severity rubric, and the spec was revised until no Critical, High or Medium findings remained open. Attack details are not published.
