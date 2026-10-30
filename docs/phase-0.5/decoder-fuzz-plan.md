# Decoder robustness (2.5): status and plan

## Status in phase 0.5

Criterion 2.5 is **not measured**, and it will stay that way in phase 0.5 on purpose. The repository has no image decoder: the three accepted formats (PNG, JPEG and WebP, see [images](../../spec/v0/images.md)) are not implemented, and images are off in both arms. Under [the decoder gate](../../spec/v0/conformance.md#gate.decoder) a terminal without a passing gate shows alt text for every image, so both arms are measured with images off, as 2.5 and section 3 of the [criteria](comparison-criteria.md) say. `lumen-evidence --decoder` returns 3 ("not measured") for that reason, and the run output says so.

This is recorded as phase 1 decoder work, not as a gap in the comparison. Nothing in 2.5 can distinguish Arm A from Arm B before a decoder exists, because both arms share the decoder and the sandbox.

## What phase 1 has to build first

The fuzzing run cannot start until these exist. None of them is in the repository today.

1. **The sandboxed decoder** for the three formats, with the pinned library versions the gate is recorded against. Each library, its version, its build (compiler, flags, commit or source hash) and its enabled features make up the decoder build identity.
2. **The terminal-side image path**: the frame parser for image frames, multi-frame reassembly, the checks on decoder replies and the upload step. It has its own build identity, and the gate covers both identities.
3. **Fuzz targets** for each format, and stateful multi-frame targets for the terminal-side path (split, reordered, truncated, duplicated and oversized chunks), built with ASan and UBSan.
4. **Seed corpora** per format that include inputs from past CVEs.
5. **The sandbox escape tests**, as a named test set with a revision.
6. **A crash classifier** that counts a hang over 2 s, memory use over 512 MiB and any sanitizer report as a crash.

## The run

> **Superseded numbers.** The CPU-hour totals and the plateau in this table are the earlier decoder gate. The fuzzing quantities now live in the [Lumen Term release criteria](../release-criteria.md#decoder-fuzzing) (a floor of 250 CPU-hours per format, a stop after 168 CPU-hours with no new crash or coverage edge, a cap of 1200, continuous fuzzing, a re-fuzz on every decoder version bump); the specification keeps only the testable safety obligations. This page is the phase 0.5 plan.

When those exist, the run is the one the gate describes, once per format and per platform:

| Part | Requirement | How it is recorded |
|---|---|---|
| CPU time | at least 1000 CPU-hours per format | `cpu_hours` in the ledger |
| Crashes | none new in the last 168 CPU-hours | each crash with the CPU-hour it was found |
| Coverage | a plateau: no new edges over the last 168 CPU-hours | cumulative edges over CPU-hours |
| Seeds | past-CVE inputs included | named in the run record |
| Sandbox | the escape tests pass in full | test set revision, passed and total |
| Identities | decoder and terminal image path | `decoder_build` and `image_path_build` in the ledger |

The fuzzing driver writes a ledger in the format `lumen-evidence --decoder` reads ([5.6](comparison-criteria.md)), and that mode applies the thresholds, so the verdict does not depend on who reads the numbers. A changed decoder or image-path identity invalidates the pass and the run repeats for the new identity. Fuzzing continues after release, and a new crash blocks release.

**Size, as arithmetic and not a measurement.** Three formats at 1000 CPU-hours each is 3000 CPU-hours of fuzzing per platform, before the plateau and the 168-hour crash-free window are met. On 32 cores that is about four days of wall time; the plateau may take longer. The ledger, not this estimate, is the record.

## What the results say until then

- Every result states 2.5 as not measured, with this reason, and that images are off in the arm measured.
- The comparison's decision rule does not depend on 2.5, which is a gate on turning images on, not a criterion that separates the arms.
- Turning images on in either arm needs a passing decoder gate on that platform first.

## Out of scope here

Choosing the decoder libraries, writing the sandbox and building the fuzz targets are phase 1 tasks with their own review. This page changes no code and no spec text.
