# Conformance vectors

Language-neutral test vectors for the Lumen wire framing and its numeric limits.
Any implementation can run them. The Rust runner is `crates/lumen-conformance`
and runs in the normal workspace tests.

The framing layout the vectors use is the experiment profile (see the detector
crate), not a normative encoding. Update the vectors when the profile changes.

## Files

| File | Covers | Status |
| --- | --- | --- |
| `vectors/framing.json` | Introducers, split-safe detection, failed-header rule, UTF-8 continuation, truncation | run |
| `vectors/limits.json` | Frame size, 4 KiB string cap, 256-byte escaped text, Track 0 type domain separation, and a table of the numeric limits listed in the transport limits section, the security resource-limits table, the budget shares and the Track B side channel, each with its vector or the reason it has none yet | run; table rows marked `pending` have no vector yet; a row with `"covers": "model"` is checked against the budget model only |
| `vectors/budgets.json` | The pooled budgets of the security resource-limits table (blocks, grids, grid cells, scrollback, shaping cache, decoded images, GPU textures) with the local reserve and the producer and unsigned shares, the per-second rates (frames and bytes per producer, patches per block and per document), and the single-item limits (frame size, nesting depth, table, grid size, layers, image size) | run against the budget model, `crates/lumen-budget`; nothing in the terminal calls the model yet |
| `vectors/grid-isolation.json` | Grid parser: C1 and string-escape drop, 4 KiB string cap and skip state | pending: no grid parser exists yet |

## Format

Bytes are written as segments: `{"hex": "1b5f"}` or
`{"fill": "61", "count": 4096}`. A vector's `input` is a list of segments. Each
segment is one read, so a runner feeds them as written, then as one read, then
one byte per read, then split at every position for inputs up to 4 KiB. All
runs must give the same result.

Detector `events` are, in order: `output` (ordinary bytes for the VT parser;
adjacent output is joined), `escaped` (the header of a rejected candidate, shown
as escaped text: the 13 bytes after the introducer, fewer if the stream ends or
an introducer starts among them; adjacent escaped is joined), `frame` (`kind`, `payload`), `rejected` (the
introducer and header of a refused candidate) and `truncated` (payload received
before the stream ended, or before an introducer plus the full tag inside an
unsealed payload, where scanning resumes at that introducer). The stream ends after the last read.

Track 0 vectors give a `kind`, a `payload` and an `expect` of `{"ok": ..}` or
`{"error": ..}` with one of `sealed_kind`, `unknown_kind`, `malformed`,
`not_utf8`, `string_too_long`, `bad_level`, `control`.

## Budget vectors

`budgets.json` has three lists. All sizes are bytes; an amount past 2^53 is a decimal string.

- `ledger_vectors`: a list of `ops` run in order against one document. `reserve` (`resource`, `who`, `amount`, `expect`, optional `tag`), `release` (`tag`) and `used` (`resource`, `expect`). `who` is `local`, `signed:<id>` or `unsigned:<id>`. `expect` is `ok` or the first rule the reservation breaks, in this order: `over_capacity`, `local_reserve`, `producer_share`, `unsigned_share`. `panes` is the number of visible panes under a multiplexer.
- `rate_vectors`: `frame` (`who`, `at_ms`, `bytes`, `expect`) and `patch` (`block`, `at_ms`, `n`, `expect`), each with an optional `count` to repeat it. Windows slide over 1000 ms; a refused amount is not counted; a clock that goes backwards counts as no time passing.
- `item_vectors`: one check each (`frame_size`, `nesting`, `table`, `grid`, `layers`, `image`).

The shares are integers: a quarter of each budget, rounded up, is kept for local use; one producer holds at most half of the rest, rounded down; unsigned producers hold at most a quarter, rounded down, between them, and that quarter divided by the visible panes each. The expected values in the file were worked out from the spec's numbers and not by running the model.

## Running

    cargo test -p lumen-conformance -- --nocapture

The output lists the limits that still have no vector, with the reason.
