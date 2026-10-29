#!/usr/bin/env bash
# Decoder fuzz campaign driver (the decoder gate): runs the fuzz targets in
# slices, records coverage samples and crashes per format, and writes the
# ledger that `lumen-evidence --decoder` judges.
#
#   scripts/decoder-fuzz.sh --out DIR [--workers N] [--slice-seconds S]
#       [--slices K] [--formats png,jpeg,webp] [--seeds DIR] [--image-path-build ID]
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
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --workers) WORKERS="$2"; shift 2 ;;
    --slice-seconds) SLICE="$2"; shift 2 ;;
    --slices) SLICES="$2"; shift 2 ;;
    --formats) FORMATS="$2"; shift 2 ;;
    --seeds) SEEDS="$2"; shift 2 ;;
    --image-path-build) IMAGE_PATH_BUILD="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
TOOLS=(cargo run -q --locked -p lumen-decoder-tools --)
cd "$ROOT"

[ -n "$SEEDS" ] || { SEEDS="$OUT/seeds"; cargo run -q --locked -p lumen-decoder-tools --example seeds -- "$SEEDS"; }

# Identity of what is fuzzed, recorded once per call so a changed build shows.
FUZZ_FLAGS="cargo-fuzz asan; libfuzzer -timeout=2 -rss_limit_mb=$RSS_MB; nightly=$NIGHTLY"
DECODER_BUILD="$("${TOOLS[@]}" identity "$ROOT" "$FUZZ_FLAGS")"
echo "$DECODER_BUILD" > "$OUT/decoder_build"
git -C "$ROOT" rev-parse HEAD > "$OUT/commit" 2>/dev/null || true

kind_of() { # artifact file -> crash kind word
  case "$(basename "$1")" in
    timeout-*) echo hang ;;
    oom-*) echo memory ;;
    *) (cd "$ROOT/crates/lumen-decoder" && cargo +"$NIGHTLY" fuzz run "$2" "$1" -- -rss_limit_mb="$RSS_MB" 2>&1 | head -c 200000 > "$OUT/replay.log" || true)
       "${TOOLS[@]}" classify "$OUT/replay.log" || true ;;
  esac
}

IFS=',' read -ra FMTS <<< "$FORMATS"
for f in "${FMTS[@]}"; do
  target="decode_$f"; dir="$OUT/$f"; mkdir -p "$dir/corpus"
  cp -n "$SEEDS/$f"/* "$dir/corpus/" 2>/dev/null || true
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
"${TOOLS[@]}" ledger fz-1 "$DECODER_BUILD" "$IMAGE_PATH_BUILD" "$ESCAPE" "${ENTRIES[@]}" > "$OUT/ledger.json"
echo "ledger: $OUT/ledger.json"
