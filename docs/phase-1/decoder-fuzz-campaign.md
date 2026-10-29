# Decoder fuzz campaign: tools and how to run it

This page describes the pieces that exist for the [decoder gate](../../spec/v0/conformance.md#gate.decoder) and what is still missing. It follows [the fuzz plan](../phase-0.5/decoder-fuzz-plan.md) and [the decoder contract](decoder-contract.md). Nothing here shows that the gate holds: the smoke run below is seconds long, and the gate needs 1000 CPU-hours per format.

## What exists

| Piece | Where | Notes |
|---|---|---|
| Fuzz targets | `crates/lumen-decoder/fuzz` | `decode_png`, `decode_jpeg`, `decode_webp` call the decoder as the worker does, with the worker's capped allocator; `reply_validate` drives the host's reply validation with arbitrary bytes and checks what it promises. Built with `cargo fuzz` on nightly, with AddressSanitizer. |
| Seeds | `cargo run -p lumen-decoder-tools --example seeds -- DIR` | Images from real encoders: PNG in 8 colour types and depths plus a 1 pixel image, JPEG in three sizes plus grayscale, WebP lossless with and without alpha. A test decodes every one with the real decoder. |
| Crash classifier | `decoder-fuzz-tools classify LOG` | Hang, memory, sanitizer, panic or other, from a libFuzzer log. Tested against excerpts of real logs in `crates/lumen-decoder-tools/tests/fixtures`. |
| Build identity | `decoder-fuzz-tools identity ROOT FLAGS` | SHA-256 over the decoder sources (not the test-only worker), its manifest, the lockfile, `rustc -vV` and the flags. |
| Campaign driver | `scripts/decoder-fuzz.sh` | Runs slices per format, keeps corpora, coverage samples and crashes, runs the G4 suite for the sandbox escape result and writes `ledger.json`. |
| Ledger | `decoder-fuzz-tools ledger ...` | The format `lumen-evidence --decoder` reads. A test builds ledgers with these tools and has the real judge decide them. |

Run a short campaign and judge it:

```sh
NIGHTLY=nightly-YYYY-MM-DD scripts/decoder-fuzz.sh --out OUT --workers 2 --slice-seconds 20 --slices 2
cargo run -p lumen-evidence -- --decoder --ledger OUT/ledger.json --commit "$(git rev-parse HEAD)" --out OUT/evidence
```

CPU-hours are wall seconds times workers, summed over slices; that is an approximation of CPU time, not a measurement of it. A longer run is the same command with larger `--slice-seconds` and `--slices`; the output directory resumes.

## The limits the fuzzer applies

- **Hang:** libFuzzer `-timeout=2`, the gate's 2 s.
- **Memory:** the targets use the worker's capped allocator, so an allocation that would take live bytes past 512 MiB aborts and is classified as memory. libFuzzer's own RSS limit is set to 2048 MiB as a runaway guard only, because it counts AddressSanitizer and corpus memory. Every `oom-*` artifact the fuzzer writes is replayed by the campaign script through the real worker (release build, no sanitizer, sandbox and limits on) with `scripts/decoder-replay.py`, which records the worker's peak resident size in `replays.jsonl`. The classifier reads that record: a peak over 512 MiB, or a worker aborted by its own heap cap, is a memory crash; anything smaller is recorded as an `asan-artifact` with its replay size and is not counted as a crash. The case that led to this: a smoke run with `-rss_limit_mb=512` reported "out-of-memory (used: 676Mb)" on a JPEG that declares 200 x 65280 pixels, and the replay script gave the real worker a peak of about 55 MiB for that input (`crates/lumen-decoder-tools/tests/fixtures/replay-oom-jpeg.json` is that record).
- **Residual limit of the memory check:** the allocator counts the Rust heap only. Memory mapped outside it (a library that calls `mmap` itself, thread stacks, the binary) is not counted by the cap. The replay's peak resident size would show it, but only for inputs the fuzzer reports as `oom-*`; an input that stays under libFuzzer's RSS limit is not replayed. In production the Linux address-space limit in the worker's sandbox covers all of it, and the fuzz run does not measure that.
- **Sanitizer:** AddressSanitizer reports. UBSan is not enabled by `cargo fuzz` by default; the gate asks for both, so enabling it is open (below).

## Not done

- **Past-CVE seeds.** None are included. They have to be collected from public reproducers for the three libraries (and their predecessors), checked for licence and size, and added with the identifier of each, before a run counts toward the gate.
- **UBSan.** The gate says ASan and UBSan; the targets are built with ASan only.
- **The terminal image path.** No frame parser, reassembly or upload step exists yet, so `image_path_build` in the ledger is written as `absent` and the stateful multi-frame targets in the plan cannot be written. The gate covers both identities, so a ledger with `absent` there must not be read as a pass.
- **Plateau.** The ledger records cumulative coverage edges; the judge applies the plateau rule. No run long enough to judge it has been done.
- **Platforms.** The gate is per platform. The smoke run was on macOS only.
- **1000 CPU-hours per format.** Not run. The plan's arithmetic is about four days of wall time on 32 cores for all three formats; the real number is whatever the ledger shows.

## What the smoke run showed

A two-slice run of 20 s with 2 workers on `png` and `webp` produced a ledger of 0.022 CPU-hours per format, edges 1537 to 1593 for PNG and 1178 to 1206 for WebP, no crashes, and the G4 suite at 33 of 33. `lumen-evidence --decoder` judged it `FAIL` (needs 1000 CPU-hours, no plateau). That is the expected result for a smoke run, and it shows the pieces fit together; it says nothing about the decoder's robustness.
