# Lumen Term release criteria

What has to hold before a Lumen Term release. The [specification](../spec/v0/conformance.md) says what any implementation must do; this page says what this implementation must have shown before it ships. The first criterion here is decoder fuzzing. More are added as they are decided.

## Decoder fuzzing

The specification's [decoder gate](../spec/v0/conformance.md#gate.decoder) keeps the testable safety obligations: bounded resources, validate before display, reject malformed input, fail safely, and the sandbox escape tests. The fuzzing quantities that used to sit beside them (a CPU-hour total, a crash-free window and a coverage plateau) are not in the specification. They are a release decision and they are stated here.

**Each image format** (PNG, JPEG and WebP), **on each platform, for the decoder build identity and the terminal image-path build identity**, must meet all of these before images are enabled in a release:

1. **A floor of 250 CPU-hours.** A format is not judged before it has had at least 250 CPU-hours of coverage-guided fuzzing.
2. **A stop rule of 168 CPU-hours.** After the floor, the run stops when the last 168 CPU-hours produced no new crash and no new coverage edge. While either still appears, the run continues. **A new crash ends the run for that build identity**: that identity cannot pass, however many crash-free hours follow, and the crash blocks release until it is fixed. The fix ships as a patched build, which has a new build identity, and rules 1 to 3 start again from zero for it. A crash cannot age out of the window.
3. **A cap of 1200 CPU-hours.** A run does not go past 1200 CPU-hours for one format. If the cap is reached before the stop rule holds, that format has not passed the run.
4. **Continuous fuzzing.** Fuzzing does not end at release. It continues against the shipped decoder and image-path builds, and a crash it finds is handled like any other: it blocks the next release and the fix ships as a patched build.
5. **A re-fuzz on every decoder version bump.** A change to the decoder build identity (a library version, the compiler, the flags or the lockfile) or to the terminal image-path identity invalidates the earlier pass for the formats it touches, and rules 1 to 3 run again for them. Until they pass, the alt text is shown, as the [image unlock rule](../spec/v0/images.md#images.unlock) already requires.

**What counts as a crash** is what the specification names: a decode over 2 s of CPU time per image (the clock the image rules use), memory use over 512 MiB, and a report from the checking the decoder's language provides (for the Rust decoders here, memory-error sanitizer reports and the overflow and assertion panics that the fuzz build leaves on; an undefined-behaviour sanitizer has no counterpart for them). The fuzz run enforces the time bound with a 2 s wall-clock timeout, which is at least as strict: a decode that used more than 2 s of CPU has taken more than 2 s of wall time, so it is caught, and a decode that only waited is reported conservatively. A crash is new if the classifier has not recorded it before in that run's ledger.

**Seeds.** Seed corpora that include inputs from past vulnerabilities are recommended. Where the chosen libraries have none, regression inputs that their upstream projects keep for bugs they fixed are used and named as such; they are not presented as vulnerability reproducers.

**The unit.** A CPU-hour here is the unit the [fuzz campaign](phase-1/decoder-fuzz-campaign.md) records: wall-clock seconds multiplied by the number of fuzzing workers, summed over the run. That is an approximation of CPU time and not a measurement of it, and it can overstate the CPU time actually used (a worker that waits still counts). The floor, the stop rule and the cap are all stated in that unit, so a run is judged by what the ledger records, and a run that wants the thresholds to mean true CPU time must record true CPU time instead and say so.

**The record** is the ledger the [fuzz campaign](phase-1/decoder-fuzz-campaign.md) writes: CPU-hours per format, each crash with the CPU-hour it was found, the cumulative coverage edges over CPU-hours, the test set revision of the sandbox escape tests, and the two build identities.

### Status

Not met. A guarded campaign on Linux passed the floor for both formats and did not reach the stop rule: JPEG recorded 378 CPU-hours and WebP 412. The judge has not been run on it. The details are in [the campaign result](phase-1/decoder-fuzz-campaign.md#the-guarded-campaign). The other platforms have no run.

The decoder judge in the comparison harness (`lumen-evidence --decoder`), the campaign's gate status and its gate-run defaults apply this rule, as does their description in [the comparison criteria](phase-0.5/comparison-criteria.md#56-decoder-mode). The judge reads one ledger, so it judges one build identity; it fails a ledger with any crash, with fewer than 250 CPU-hours, with more than 1200, or without a coverage reading from before the last 168 CPU-hours that already had every edge seen. It does not judge continuous fuzzing or a re-fuzz after a version bump (rules 4 and 5): those are about what happens after a ledger, not inside one.

### The release check

`scripts/release-check.sh` is the gate a release goes through, and `.github/workflows/release-check.yml` runs it on a `v*` tag or by hand. It refuses unless all of these hold, and a missing input is a refusal, never a skip:

1. The personal-identifier scan (`scripts/check-public-text.sh`) passes.
2. The decoder fuzz evidence is committed: `release/decoder-ledger.json` (the ledger above) and `release/decoder-fuzz-flags.txt` (the exact flags string the campaign gave the identity tool, which the ledger does not carry).
3. The ledger records no crash. A crash stays open until a patched build, with its own identity and its own ledger, replaces it (rule 2).
4. The ledger's decoder and image-path build identities equal the identities of the tree being released, recomputed with the recorded flags (rule 5).
5. `lumen-evidence --decoder` passes the ledger against the thresholds above.

There is no waiver switch, and the check does not tag, publish or release anything. `scripts/test-release-check.sh` holds a positive case and a negative case for each rule. Until the evidence in item 2 is committed the check refuses, which is the intended state while the [status](#status) is not met.

## Signed macOS binaries

The macOS release gates (task-port denial and non-dumpability) are judged on the binaries a release ships, so those binaries have to be signed first. The `macos-signed` job in `.github/workflows/release-check.yml` builds `lumen-term` and `lumen-holder`, signs them with `scripts/sign-macos-binaries.sh`, checks them with `scripts/verify-macos-signed.sh`, and keeps them as the `lumen-macos-signed` workflow artifact for the gate runs.

- The signature is made with the hardened runtime and no entitlements. The verifier refuses a binary that is unsigned, whose signature does not verify, that lacks the hardened runtime flag, that carries `get-task-allow`, that is signed ad hoc, or that is not a Developer ID Application signature with a team identifier.
- The Developer ID certificate comes from the repository secrets `APPLE_CERTIFICATE_P12` and `APPLE_CERTIFICATE_PASSWORD`, is imported into a throwaway keychain that is deleted on exit, and is never printed. With either secret missing the job refuses; there is no unsigned fallback and no waiver.
- The job does not notarize, publish or release anything, and it leaves the decoder fuzz checks above untouched.
- `scripts/test-macos-signing.sh` runs on macOS in CI and in the job, with ad-hoc signatures instead of a certificate. It proves the verifier refuses every non-release shape; it cannot make a Developer ID signature, so that path is exercised only by the job itself once the secrets exist.
