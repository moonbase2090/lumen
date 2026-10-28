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
| `vectors/limits.json` | Frame size, 4 KiB string cap, 256-byte escaped text, Track 0 type domain separation, and a table of the numeric limits listed in the transport limits section, the security resource-limits table, the budget shares and the Track B side channel, each with its vector or the reason it has none yet | run; table rows marked `pending` have no vector yet |
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

## Running

    cargo test -p lumen-conformance -- --nocapture

The output lists the limits that still have no vector, with the reason.
