+++
title = "Capability negotiation and versioning"
weight = 110
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Capability negotiation and versioning {#caps}

## Versions {#caps.version}

The protocol version is `major.minor`. A minor version adds optional features. A major version may break compatibility.

## Capabilities {#caps.list}

v0 defines the capabilities `images`, `tables`, `rgb-color`, `cell-spans`, `side-channel`, `hyperlinks` and `clipboard-write`. An advertisement (including `LUMEN=0`) is only a hint. The terminal decides by its policy, and nothing is ever granted because of the advertisement alone.

## Grants {#caps.grants}

Capabilities are granted per sealed layer, at shell start, by local policy and chrome prompts. The default grant for a remote layer is text, tables and links.

## Clipboard write {#caps.clipboard}

`clipboard-write` requires a confirmation drawn by the terminal that shows the exact bytes. The confirmation covers one write or the layer. Track 0 can never write the clipboard. See [Clipboard](@/spec/v0/links-and-clipboard.md#clip.write).

## Policy file {#caps.policy}

Policy is read only from a user-owned file whose mode does not allow writes by group or others. It is never read from the current directory, the environment, frames or a remote host. A file that fails these checks is ignored, with a chrome notice, and the defaults apply. A system-wide file MAY exist only for the settings that this specification allows it to set (for example [hardened mode](@/spec/v0/hardened-mode.md#hardened.enable)).
