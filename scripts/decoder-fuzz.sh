#!/usr/bin/env bash
# Decoder fuzz campaign driver (the decoder gate): runs the fuzz targets in
# slices, records coverage samples and crashes per format, and writes the
# ledger that `lumen-evidence --decoder` judges.
#
#   scripts/decoder-fuzz.sh --out DIR [--workers N] [--slice-seconds S]
#       [--slices K] [--formats png,jpeg,webp] [--seeds DIR] [--image-path-build ID] [--rss-mb MB] [--miri-record FILE]
#
# CPU-hours are wall time times workers, summed over the slices. The run
# resumes in DIR: corpora, samples and crashes are kept, so a later call adds
# more slices. Nothing here proves the gate; the ledger is judged by
# `lumen-evidence --decoder --ledger DIR/ledger.json`.
#
# Limits: libFuzzer timeout 2 s (a hang). Memory over 512 MiB is the worker's
# own cap (the capped allocator aborts the target), and the libFuzzer RSS limit
# is only a runaway guard because ASan and the corpus inflate RSS well past the
# worker's real use.
set -euo pipefail

NIGHTLY="${NIGHTLY:-nightly}"
OUT=""; WORKERS=2; SLICE=3600; SLICES=1; FORMATS="png,jpeg,webp"; SEEDS=""
IMAGE_PATH_BUILD="absent: no terminal image path exists yet"
RSS_MB=2048
MIRI_RECORD=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --workers) WORKERS="$2"; shift 2 ;;
    --slice-seconds) SLICE="$2"; shift 2 ;;
    --slices) SLICES="$2"; shift 2 ;;
    --formats) FORMATS="$2"; shift 2 ;;
    --seeds) SEEDS="$2"; shift 2 ;;
    --image-path-build) IMAGE_PATH_BUILD="$2"; shift 2 ;;
    --rss-mb) RSS_MB="$2"; shift 2 ;;
    --miri-record) MIRI_RECORD="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
TOOLS=(cargo run -q --locked -p lumen-decoder-tools --)
cd "$ROOT"

[ -n "$SEEDS" ] || { SEEDS="$OUT/seeds"; cargo run -q --locked -p lumen-decoder-tools --example seeds -- "$SEEDS"; }
# The upstream projects' regression inputs, pinned in their manifest. They are
# not CVE reproducers: the chosen libraries have no registered advisories.
REGRESSION="$ROOT/crates/lumen-decoder/fuzz/seeds-regression"

# Identity of what is fuzzed, recorded once per call so a changed build shows.
FUZZ_FLAGS="cargo-fuzz asan; overflow-checks+debug-assertions (no UBSan for Rust); libfuzzer -timeout=2 -rss_limit_mb=$RSS_MB; nightly=$NIGHTLY"
DECODER_BUILD="$("${TOOLS[@]}" identity "$ROOT" "$FUZZ_FLAGS")"
echo "$DECODER_BUILD" > "$OUT/decoder_build"
git -C "$ROOT" rev-parse HEAD > "$OUT/commit" 2>/dev/null || true

# The real worker, built without sanitizers, replays every oom-* artifact.
cargo build -q --release --locked -p lumen-decoder --bin lumen-decoder-worker
WORKER="$ROOT/target/release/lumen-decoder-worker"

kind_of() { # artifact file, target -> crash kind word
  case "$(basename "$1")" in
    timeout-*) echo hang ;;
    oom-*)
      # libFuzzer's RSS limit counts sanitizer and corpus memory. Only the real
      # worker's peak decides: over 512 MiB, or aborted by its heap cap, is a
      # memory crash; anything smaller is recorded as an ASan artifact.
      fmt="${2#decode_}"
      python3 "$ROOT/scripts/decoder-replay.py" "$WORKER" "$fmt" "$1" > "$OUT/replay.json" || echo '{}' > "$OUT/replay.json"
      (cat "$OUT/replay.json"; echo) >> "$OUT/$fmt/replays.jsonl"
      "${TOOLS[@]}" replay-kind "$OUT/replay.json" || echo other ;;
    *) (cd "$ROOT/crates/lumen-decoder" && cargo +"$NIGHTLY" fuzz run "$2" "$1" -- -rss_limit_mb="$RSS_MB" 2>&1 | head -c 200000 > "$OUT/replay.log" || true)
       "${TOOLS[@]}" classify "$OUT/replay.log" || true ;;
  esac
}

IFS=',' read -ra FMTS <<< "$FORMATS"
for f in "${FMTS[@]}"; do
  target="decode_$f"; dir="$OUT/$f"; mkdir -p "$dir/corpus"
  cp -n "$SEEDS/$f"/* "$dir/corpus/" 2>/dev/null || true
  cp -n "$REGRESSION/$f"/* "$dir/corpus/" 2>/dev/null || true
  touch "$dir/samples.jsonl" "$dir/crashes.jsonl"
  art="$ROOT/crates/lumen-decoder/fuzz/artifacts/$target"
  for _ in $(seq 1 "$SLICES"); do
    before="$(ls "$art" 2>/dev/null | sort || true)"
    done_h="$(python3 - "$dir/samples.jsonl" <<'PY'
import json,sys
h=0.0
for l in open(sys.argv[1]):
    if l.strip(): h=max(h,json.loads(l)["at_cpu_hour"])
print(h)
PY
)"
    (cd "$ROOT/crates/lumen-decoder" && cargo +"$NIGHTLY" fuzz run "$target" "$dir/corpus" -- \
        -fork="$WORKERS" -ignore_crashes=1 -max_total_time="$SLICE" -timeout=2 \
        -rss_limit_mb="$RSS_MB" -max_len=65536 > "$dir/last.log" 2>&1) || true
    edges="$(grep -E '^#[0-9]+: cov:' "$dir/last.log" | tail -1 | sed -E 's/.*cov: ([0-9]+).*/\1/' || true)"
    now_h="$(python3 -c "print($done_h + $SLICE * $WORKERS / 3600.0)")"
    if [ -n "${edges:-}" ]; then
      echo "{\"at_cpu_hour\": $now_h, \"edges\": $edges}" >> "$dir/samples.jsonl"
    else
      echo "no coverage line in $dir/last.log; slice not counted" >&2
    fi
    after="$(ls "$art" 2>/dev/null | sort || true)"
    comm -13 <(echo "$before") <(echo "$after") | while read -r a; do
      [ -n "$a" ] || continue
      k="$(kind_of "$art/$a" "$target")"
      if [ "$k" = "asan-artifact" ]; then
        echo "oom report in $f ($a) replayed under the limit; recorded in replays.jsonl, not a crash" >&2
        continue
      fi
      echo "{\"at_cpu_hour\": $now_h, \"kind\": \"${k:-other}\", \"artifact\": \"$a\"}" >> "$dir/crashes.jsonl"
      echo "crash in $f at ${now_h} CPU-hours: ${k:-other} ($a)" >&2
    done
  done
done

# The sandbox escape result: the G4 suite at this revision.
cargo test --locked -p lumen-decoder --test g4_sandbox 2>&1 | sed 's/\x1b\[[0-9;]*m//g' > "$OUT/g4.log" || true
ESC_REV="$(git -C "$ROOT" hash-object crates/lumen-decoder/tests/g4_sandbox.rs | cut -c1-12)"
ESCAPE="$("${TOOLS[@]}" escape "$OUT/g4.log" "g4-$ESC_REV")"

ENTRIES=()
for f in "${FMTS[@]}"; do
  ENTRIES+=("$("${TOOLS[@]}" format "$f" "$OUT/$f/samples.jsonl" "$OUT/$f/crashes.jsonl")")
done
"${TOOLS[@]}" ledger fz-1 "$DECODER_BUILD" "$IMAGE_PATH_BUILD" "$ESCAPE" "${ENTRIES[@]}" > "$OUT/ledger.plain.json"

# Notes that travel with the numbers (the judge ignores them): what was fuzzed,
# what was not run, and the empty past-CVE record.
cat > "$OUT/build.json" <<JSON
{"fuzzed": "default-feature build (what ships: zune-jpeg x86/neon SIMD on)", "sanitizer": "AddressSanitizer", "overflow_checks_and_debug_assertions": true, "nightly": "$NIGHTLY", "libfuzzer": "-timeout=2 -rss_limit_mb=$RSS_MB (runaway guard); 512 MiB is the worker heap cap"}
JSON
cat > "$OUT/ubsan.json" <<'JSON'
{"run": false, "reason": "Rust has no UBSan (rustc -Zsanitizer has no undefined). Nothing here is labelled UBSan. Overflow checks and debug assertions are on in the fuzz build; Miri results, if any, are in notes.miri."}
JSON
NOTES=("past_cve_seeds=$ROOT/crates/lumen-decoder/fuzz/seeds-regression/PAST_CVES.json" "build=$OUT/build.json" "ubsan=$OUT/ubsan.json")
[ -n "$MIRI_RECORD" ] || MIRI_RECORD="$ROOT/crates/lumen-decoder/fuzz/miri-record.json"
[ ! -f "$MIRI_RECORD" ] || NOTES+=("miri=$MIRI_RECORD")
"${TOOLS[@]}" annotate "$OUT/ledger.plain.json" "${NOTES[@]}" > "$OUT/ledger.json"
echo "ledger: $OUT/ledger.json"
