# Bright release criteria

What has to hold before a Bright release. The [specification](../spec/v0/conformance.md) says what any implementation must do; this page says what this implementation must have shown before it ships. The first criterion here is decoder fuzzing. More are added as they are decided.

## Decoder fuzzing

The specification's [decoder gate](../spec/v0/conformance.md#gate.decoder) keeps the testable safety obligations: bounded resources, validate before display, reject malformed input, fail safely, and the sandbox escape tests. The fuzzing quantities that used to sit beside them (a CPU-hour total, a crash-free window and a coverage plateau) are not in the specification. They are a release decision and they are stated here.

**Each image format** (PNG, JPEG and WebP), **on each platform, for the decoder build identity and the terminal image-path build identity**, must meet all of these before images are enabled in a release:

1. **A floor of 250 CPU-hours.** A format is not judged before it has had at least 250 CPU-hours of coverage-guided fuzzing.
2. **No open crash or confirmed hang.** After the floor, the format passes when no crash or confirmed hang is open for that build identity. **An open crash ends the run for that build identity**: that identity cannot pass, however many crash-free hours follow, and the crash blocks release until it is fixed. The fix ships as a patched build, which has a new build identity, and rules 1 and 2 start again from zero for it. A crash cannot age out. A raw timeout the fuzzer reports is a hang only if it is confirmed: it is replayed under the production limits (4 s wall clock, 2 s CPU, 512 MiB), and a record that stays inside them is cleared and recorded as such, with the replay kept. Any record that has not been replayed is open.
3. **No other condition on the run.** The Lumen maintainers adopted this rule in place of an earlier stop rule (a stop after 168 CPU-hours with no new crash or coverage edge, with a cap of 1200 CPU-hours that had to be reached before the stop rule held). The campaign never reached that plateau, and a coverage plateau measures how the fuzzer's search is going, not whether a finding is open. The coverage curve is still recorded and reported; it does not decide the gate, and neither does how far the run goes past the floor.
4. **Continuous fuzzing.** Fuzzing does not end at release. It continues against the shipped decoder and image-path builds, and a crash it finds is handled like any other: it blocks the next release and the fix ships as a patched build.

**What counts as a crash** is what the specification names: a decode over 2 s of CPU time per image (the clock the image rules use), memory use over 512 MiB, and a report from the checking the decoder's language provides (for the Rust decoders here, memory-error sanitizer reports and the overflow and assertion panics that the fuzz build leaves on; an undefined-behaviour sanitizer has no counterpart for them). The fuzz run enforces the time bound with a 2 s wall-clock timeout, which is at least as strict: a decode that used more than 2 s of CPU has taken more than 2 s of wall time, so it is caught, and a decode that only waited is reported conservatively. A crash is new if the classifier has not recorded it before in that run's ledger.

**Seeds.** Seed corpora that include inputs from past vulnerabilities are recommended. Where the chosen libraries have none, regression inputs that their upstream projects keep for bugs they fixed are used and named as such; they are not presented as vulnerability reproducers.

**The unit.** A CPU-hour here is the unit the [fuzz campaign](phase-1/decoder-fuzz-campaign.md) records: wall-clock seconds multiplied by the number of fuzzing workers, summed over the run. That is an approximation of CPU time and not a measurement of it, and it can overstate the CPU time actually used (a worker that waits still counts). The floor is stated in that unit, so a run is judged by what the ledger records, and a run that wants the thresholds to mean true CPU time must record true CPU time instead and say so.

**The record** is the ledger the [fuzz campaign](phase-1/decoder-fuzz-campaign.md) writes: CPU-hours per format, each crash with the CPU-hour it was found, the cumulative coverage edges over CPU-hours (for the record), the replay that cleared each raw timeout, the test set revision of the sandbox escape tests, and the two build identities.

### Status

**Met for the current build identities.** Image testing is complete, and images are on by default on Linux and macOS. Windows is not supported for images in v0 and has no record, so a Windows build shows the alt text ([why](../spec/v0/platforms.md#platforms.windows)).

The decoder ledger records the guarded-campaign fuzz runs: PNG 352 CPU-hours, JPEG 378 and WebP 412, each with no open crash or confirmed hang (930 raw JPEG timeout records were replayed under the production limits and none exceeded a limit; the PNG run is on the build before the WebP guard). What the record does not claim:

- The runs were on Linux, on earlier identities. The decoder's sandbox and manifest changed after them (Windows support), and the frame parser, the reassembly and the terminal's image host were never fuzzed. The sandbox escape suite (34 tests, revision `g4-c77545c64b3c`) has run on macOS and runs in CI on Linux and Windows.
- A release identity is the hash of the listed decoder and image-path sources and the lockfile, **without the compiler and flags** (`decoder-fuzz-tools identity ROOT source-only [decoder|image-path]`), so that one identity serves every platform. A toolchain change alone does not change the identities. A change to a source or the lockfile does: the identities in the committed records stop matching, the test `release_gate` fails, the release check refuses and the terminal shows the alt text, until records for the new identities are committed.
- For the decoder memory gate, these memory tests exist and run in CI: a declared size over the limits is refused before anything is reserved, the reservation shrinks to the decoded size, queued images count against the budget, an upload past the texture budget is never made, the worker's 512 MiB limit stops a worker that hogs memory, a reply larger than its reservation is a crash, and a flood on standard output is cut off at the reservation. That is a unit-level measurement and not a fuzz result.

The records are `release/gate-records.txt` (one line per supported platform, Linux and macOS: the two identities and both gates), `release/decoder-ledger.json` (the ledger the judge reads) and `release/decoder-fuzz-flags.txt` (the string `source-only`). The terminal's build script reads the records, so a build whose identities have a record shows images and any other build shows the alt text. The setting `images = false` always shows the alt text. The details of the runs are in [the campaign result](phase-1/decoder-fuzz-campaign.md#the-guarded-campaign).

The decoder judge in the comparison harness (`lumen-evidence --decoder`), the campaign's gate status and its gate-run defaults apply this rule, as does their description in [the comparison criteria](phase-0.5/comparison-criteria.md#56-decoder-mode). The judge reads one ledger, so it judges one build identity (the committed ledger names the current one); it fails a ledger with fewer than 250 CPU-hours or with any record not marked cleared. It reports the coverage reading and the run length and does not judge on them. It does not judge continuous fuzzing (rule 4): that is about what happens after a ledger, not inside one.

### The release check

`scripts/release-check.sh` is the gate a release goes through, and `.github/workflows/release-check.yml` runs it on a `v*` tag or by hand. It refuses unless all of these hold, and a missing input is a refusal, never a skip:

1. The personal-identifier scan (`scripts/check-public-text.sh`) passes.
2. The decoder fuzz evidence is committed: `release/decoder-ledger.json` (the ledger above) and `release/decoder-fuzz-flags.txt` (the exact flags string the campaign gave the identity tool, which the ledger does not carry).
3. The ledger records no open crash or confirmed hang (a record is cleared only if it is marked `"cleared": true`). An open one stays open until a patched build, with its own identity and its own ledger, replaces it (rule 2).
4. The ledger's decoder and image-path build identities equal the identities of the tree being released, recomputed with the recorded flags.
5. `lumen-evidence --decoder` passes the ledger against the thresholds above.

There is no waiver switch, and the check does not tag, publish or release anything. `scripts/test-release-check.sh` holds a positive case and a negative case for each rule. The evidence in item 2 is committed in `release/`, so the check passes for the current tree and refuses when a decoder or image-path source or the lockfile changes without new records. `crates/lumen-decoder-tools/tests/release_gate.rs` runs the same comparisons in the test suite.

## Signed macOS app

The macOS release gates (task-port denial and non-dumpability) are judged on the app a release ships, so the app has to be signed first. The `macos-signed` job in `.github/workflows/release-check.yml` builds the internal `lumen-term` target with the `window` feature, packages it as `Bright.app` with the `bright` executable, and places `lumen-decoder-worker` and `lumen-holder` beside it. The job signs the nested executables and app with identifiers rooted at `app.brightterm.Bright`, verifies them, and keeps the app as the `bright-macos-signed` workflow artifact for the gate runs.

- The signature is made with the hardened runtime and no entitlements. The verifier refuses a binary that is unsigned, whose signature does not verify, that lacks the hardened runtime flag, that carries `get-task-allow`, that is signed ad hoc, or that is not a Developer ID Application signature with a team identifier.
- The Developer ID certificate comes from the repository secrets `APPLE_CERTIFICATE_P12` and `APPLE_CERTIFICATE_PASSWORD`, is imported into a throwaway keychain that is deleted on exit, and is never printed. With either secret missing the job refuses; there is no unsigned fallback and no waiver.
- `scripts/verify-release-set.sh` refuses a release set (before and after signing) that lacks `Bright.app`, its `bright` executable, `lumen-decoder-worker` or `lumen-holder`; has the wrong bundle identity; holds a terminal built without the window feature; or holds the test-only `lumen-decoder-hostile`. Bright looks for the worker beside its own executable, so a set without it shows every image as alt text. `scripts/test-verify-release-set.sh` covers each refusal and runs in CI.
- The job does not notarize, publish or release anything, and it leaves the decoder fuzz checks above untouched.
- `scripts/test-macos-signing.sh` runs on macOS in CI and in the job, with ad-hoc signatures instead of a certificate. It proves the verifier refuses every non-release shape; it cannot make a Developer ID signature, so that path is exercised only by the job itself once the secrets exist.
