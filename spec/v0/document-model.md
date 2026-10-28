+++
title = "Document model"
weight = 20
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Document model {#doc}

## Structure {#doc.structure}

A session contains a document, and the document is an ordered tree of blocks. Each block has a stable id, a kind, attributes, children and a state: `open`, `streaming` or `closed`.

## Append-mostly content {#doc.append-mostly}

Content is append-mostly. A block can be patched by id until it is closed. After that it is immutable.

## Revisions and selection {#doc.revisions}

Every block carries a revision number that increments on each patch. A selection records (block id, revision, offsets). If any selected block's revision has changed since the selection was made, copy MUST fail with a visible notice.

## Command blocks {#doc.command-blocks}

A command block wraps the prompt, input, output and exit status, building in the meaning of OSC 133. Command boundaries and exit status MUST come only from authenticated frames sent by the terminal-owned shell integration, or from the terminal's own [unverified boundaries](@/spec/v0/trust.md#trust.unverified-boundaries). OSC 133 seen in grid output MUST NOT create blocks or exit status.

## Output kinds {#doc.output}

A command's output can be rich blocks, a grid block or both. Any program can emit inert rich blocks as [unverified output](@/spec/v0/trust.md). Only sealed shell integration can emit authority frames such as command structure and exit status. Applications cannot emit sealed frames in v0.

## Producers {#doc.producers}

Every block records its producer (a layer id), and the chrome shows it. Ids for blocks and images are scoped per producer, so one producer cannot patch or replace another's content. The terminal assigns layer ids at handshake and binds them into every key derivation. A peer cannot choose or claim a layer id.

## Clean text form {#doc.clean-text}

A clean text form (plain text plus a semantic outline) can be derived for copy, search and logs. It MUST NOT contain frames, authentication tags, keys or sequence state. By default, copy and search MUST cover only visible content (see [visibility](@/spec/v0/text-runs.md#runs.visible)).

## No secrets in the document {#doc.no-secrets}

The document store MUST NOT hold authentication tags, keys or tokens as block attributes. Authentication state lives only in the transport layer and is never saved with a session.
