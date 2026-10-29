# Image decoder: process contract and library choice

This page defines the contract between the terminal and the out-of-process image decoder, and proposes the decoding libraries. It implements [images](../../spec/v0/images.md) and is what the sandbox test suite and the fuzz run ([plan](../phase-0.5/decoder-fuzz-plan.md)) are written against. It changes no code and no spec text. Where it says "proposed", the choice is open for review.

## Process model

- The decoder is a separate executable, `lumen-decoder-worker`, started by the terminal for **one image per process**. It exits after one reply. There is no pooled or long-lived worker, so no state survives from one image to the next.
- The terminal starts it in its own process group (session on Unix, job object on Windows) so that the whole group can be killed.
- The worker is invoked with no arguments and an empty environment. It reads one request from stdin and writes one reply to stdout. Stdin and stdout are the only descriptors it keeps.
- At most 1 worker per producer and 2 across the terminal run at once ([concurrency](../../spec/v0/images.md#images.concurrency)).

## Request (terminal to worker, stdin)

All integers are little-endian.

| Field | Size | Meaning |
|---|---|---|
| magic | 4 | `LDEC` |
| version | 1 | `1` |
| format | 1 | `1` PNG, `2` JPEG, `3` WebP |
| reserved | 2 | zero |
| hint_width | 4 | declared width, a hint only |
| hint_height | 4 | declared height, a hint only |
| payload_len | 8 | length of the image bytes that follow |
| payload | `payload_len` | the encoded image |

The terminal sets `format` from the sniffed signature, not from the producer's claim. The worker re-checks the signature and answers `format` if they disagree. The worker rejects a request whose `payload_len` is over its input cap (see limits) before it reads the payload, and a request with extra bytes after the payload.

## Reply (worker to terminal, stdout)

| Field | Size | Meaning |
|---|---|---|
| magic | 4 | `LDER` |
| status | 1 | see the table below |
| reserved | 3 | zero |
| width | 4 | pixels |
| height | 4 | pixels |
| stride | 4 | bytes per row, exactly `width * 4` |
| data_len | 8 | exactly `stride * height` |
| data | `data_len` | 8-bit premultiplied RGBA, top-down, first frame only |

On any non-zero status, `width`, `height`, `stride` and `data_len` are zero and no data follows.

| Status | Meaning |
|---|---|
| 0 | ok |
| 1 | malformed request |
| 2 | unsupported or mismatched format |
| 3 | over a size limit |
| 4 | corrupt image |
| 5 | internal failure |

Statuses 1 to 5 are not crashes: the terminal shows the alt text. A crash is anything else: death by signal, a nonzero exit without a reply, a timeout, an over-limit stdout or a reply that fails validation. A crash puts that producer's images on alt text for the rest of the command ([concurrency](../../spec/v0/images.md#images.concurrency)).

## What the terminal does with a reply

The worker is hostile by assumption, and the terminal does not rely on any of the above being true.

1. **Before it starts the worker**, the terminal reserves from the session budget the bytes the image may use, from the hint: `hint_width * hint_height * 4`, in checked 64-bit arithmetic, and refuses the image if the hint is over 32 megapixels, over 65535 on a side or zero. The reservation is released when the worker is done.
2. It reads **at most `hint_width * hint_height * 4 + 4096` bytes** of stdout, and stops reading at that cap ([budget](../../spec/v0/images.md#images.budget)). It never grows a buffer on the worker's say-so.
3. It validates `magic`, `status`, `width`, `height`, `stride` and `data_len` ([validate](../../spec/v0/images.md#images.validate)): `width` and `height` are each in 1 to 65535, their product is at most 32 megapixels, `stride == width * 4`, `data_len == stride * height`, and `data_len` is within the reservation. The count of bytes actually received must equal `data_len` exactly. All of it in checked 64-bit arithmetic.
4. Only after step 3 does it allocate or upload anything. A mismatch is a crash.

The worker may decode an image whose true size differs from the hint. It is fine for the true size to be **smaller** than the hint. It is a crash if it is larger than the reservation.

## Limits and the sandbox

| Limit | Value | Enforced by |
|---|---|---|
| Address space (memory) | 512 MiB | resource limit set in the worker before it reads any input, and the terminal's own check on process memory where the platform allows |
| CPU time | 2 s per image | CPU resource limit, plus a 4 s wall-clock timeout in the terminal for a worker that sleeps |
| Input size | 64 MiB | worker, before reading the payload |
| Output | 32 megapixels, 65535 per side | worker, on the header, before it allocates pixels |
| Open files | stdin, stdout and stderr only | descriptors closed or limited to zero further files |
| Processes | none beyond itself | process-count limit where supported, and a kill of the whole group by the terminal |
| Filesystem | none | sandbox profile |
| Network | none | sandbox profile |
| Stderr | at most 4 KiB kept, the rest discarded | terminal; a flood is read and dropped, never buffered, and does not block the worker |

The sandbox is entered **before the worker reads a byte of input**:

- **Linux:** resource limits, `no_new_privs`, then a seccomp-bpf filter that allows only the system calls the decoder needs (read, write, memory mapping, exit, futex, clock, signal return) and kills the process on anything else. It does not allow `open`, `socket`, `execve` or `fork`/`clone` of a process.
- **macOS:** resource limits, then a Seatbelt profile that denies by default. The worker uses the bundled libraries, never ImageIO ([output](../../spec/v0/images.md#images.output)).
- **Windows (test profile only in this phase):** a restricted token, a job object with the same memory and CPU limits and no child processes, and no handle other than the pipes. It is exercised by tests, not shipped, until Windows is a supported platform.

A failure to enter the sandbox is a hard failure: the worker exits before reading, and the terminal treats it as a crash. There is no run without the sandbox.

**Unsafe code.** Entering the sandbox needs system calls that Rust cannot make safely. They live in one module named `ffi_sandbox`, the only place in the crate that is allowed `unsafe`, as [the review policy](../../REVIEW_POLICY.md) describes. Every other module keeps `#![forbid(unsafe_code)]`, and the module is small enough to read in full. The module is justified in the PR that adds it.

## Decoding

- First frame only. Animation, extra frames, and all metadata (EXIF, ICC, text chunks) are dropped inside the worker. No colour management and no orientation rotation.
- Output is converted to 8-bit premultiplied RGBA. 16-bit and palette inputs are converted by the worker.
- The worker reads the header first, applies the size limits to the **decoded** dimensions in checked 64-bit arithmetic, and only then allocates the output buffer. It does not trust `hint_width` or `hint_height`.
- No `unsafe` in the decoding path, and no C library in the worker.

## Proposed libraries

All three are pure Rust, so the decoder build has no C code to compile, no system library to pick up, and the same code on Linux, macOS and Windows. Versions are those on the crate registry when this page was written; the lockfile hash, not this page, is the record, and the decoder build identity is computed from it ([unlock](../../spec/v0/images.md#images.unlock)).

| Format | Crate | Version | Licence | Notes |
|---|---|---|---|---|
| PNG | `png` | 0.18.1 | MIT OR Apache-2.0 | Widely used. Has decode limits that the worker sets explicitly. |
| JPEG | `zune-jpeg` | 0.5.15 (the last non-release-candidate) | MIT OR Apache-2.0 OR Zlib | Pure Rust, widely used, and actively maintained. The newest release is a release candidate, which this page does not propose. |
| WebP | `image-webp` | 0.2.4 | MIT OR Apache-2.0 | Pure Rust, decode only is used. |

All three licences are covered by `deny.toml`, as `MIT`, `Apache-2.0` or `Zlib`.

**Why not the alternatives.**
- `jpeg-decoder` is also pure Rust, but is the older of the two JPEG options; `zune-jpeg` is the one more projects now pick. This is a judgment about maintenance, not a measured difference, and it is open for review.
- Binding to `libpng`, `libjpeg-turbo` or `libwebp` gives the best-known fuzzing history, and the largest memory-unsafe surface. It would add C to the build and to the build identity, and on macOS a risk of linking the system copy. Not proposed for phase 1.
- The system ImageIO framework is excluded by the spec.

**What this does not claim.** No library here has been fuzzed or measured by this project. The pure-Rust choice reduces memory-corruption risk, and does not remove hangs, huge allocations, or logic errors, which is why the limits above are enforced from outside the library and why the fuzz run in the plan is still required.

## What the tests and the fuzz run use

- **Sandbox suite (G4).** Runs the real worker and the real terminal-side runner. It covers: a hint that lies about size in both directions, a decoded size past 32 megapixels or 65535 on a side, a 64-bit overflow in `width * height * 4`, a worker that writes more or less than `data_len`, a stderr flood, a worker that hangs, a worker that spawns a child (the group is killed, nothing survives), an attempt to open a file or a socket from inside the sandbox, and peak terminal memory under a lying worker. Each runs on Linux and macOS; the Windows test profile runs the subset its platform supports.
- **Fuzz targets.** One per format, calling the worker's decode function directly, built with ASan and UBSan, plus stateful targets for the terminal-side reply validation. The build identities are the ones in the unlock rule, and a changed lockfile re-runs the gate.

## Out of scope here

The terminal's frame parser for image frames, the upload step and the GPU texture budget are the terminal-side image path, tracked on their own. This page fixes only the interface they use.
