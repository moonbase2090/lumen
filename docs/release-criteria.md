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

**The record** is the ledger the [fuzz campaign](phase-1/decoder-fuzz-campaign.md) writes: CPU-hours per format, each crash with the CPU-hour it was found, the cumulative coverage edges over CPU-hours, the test set revision of the sandbox escape tests, and the two build identities.

### Status

Not met. No run has reached the floor. The fuzz campaign tooling exists, and a short smoke run has been done on one platform.

The judge in `lumen-evidence --decoder` and its description in [the comparison criteria](phase-0.5/comparison-criteria.md) still apply the earlier numbers (1000 CPU-hours, a 168-hour crash-free window and a plateau) until they are changed to this rule. A pass or fail from them is not a pass or fail of this criterion.
