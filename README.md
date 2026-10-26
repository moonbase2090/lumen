# Lumen

Lumen is an open protocol for structured terminal content: blocks, text runs, layout and events. **Bright Term** is its Rust reference terminal.

Terminals still render a 1970s character grid. Lumen treats a session as a document. Prose, logs and tool replies get proportional fonts and browser-style layout, while legacy apps keep running untouched.

## Core ideas
- **Blocks, not scrollback.** Each command and its output is a block.
- **Rich blocks.** Opt-in headings, real tables, images, links and collapsible sections.
- **Grid blocks.** A layout element with its own cols x rows, backed by a PTY sized to the block. vim, htop, ssh and existing TUIs run inside, alt-screen apps grow to the viewport and collapse back to a snapshot on exit, and old and new apps share one document.
- **Not backwards compatible by design**, with a native middle path for legacy apps.
- **GPU text** with full shaping, ligatures and emoji, and no web engine.

## Principles
1. **Secure by default.** Untrusted output must never be able to forge protocol frames. The threat model is part of the spec.
2. **Evidence over opinion.** Every design fork is prototyped both ways before v0 picks one.
3. **Developed in public.** Design, spec and code happen in the open.

## Status
Concept and spec stage: no code yet. Early work runs experiments on transport (PTY framing vs a local side channel), frame authentication, mode detection, layout scope and copy/paste semantics.

## Projects
- **Lumen**: the protocol specification
- **Bright Term**: Rust reference terminal
- **Lunatui**: the first native client library

## Security
Please don't open public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).

## License
Code is dual-licensed under MIT or Apache-2.0, at your option. The Lumen specification is licensed under CC-BY-4.0.
