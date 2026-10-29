#!/usr/bin/env bash
# Decoder fuzz campaign driver (the decoder gate): runs the fuzz targets in
# slices, records coverage samples and crashes per format, and writes the
# ledger that `lumen-evidence --decoder` judges.
#
#   scripts/decoder-fuzz.sh --out DIR [--workers N] [--slice-seconds S]
#       [--slices K] [--formats png,jpeg,webp] [--seeds DIR] [--image-path-build ID] [--rss-mb MB] [--miri-record FILE]
#       [--target-cpu-hours H] [--restore-cmd CMD] [--sync-cmd CMD] [--finish-cmd CMD]
#
# CPU-hours are wall time times workers, summed over the slices. The run
# resumes in DIR: corpora, samples and crashes are kept, so a later call adds
# more slices. Nothing here proves the gate; the ledger is judged by
# `lumen-evidence --decoder --ledger DIR/ledger.json`.
#
# For a one-time (spot) VM that can disappear:
#   --target-cpu-hours H   run each format until it has H CPU-hours, however many
#                          calls that takes (replaces --slices)
#   --restore-cmd CMD      run once at the start to fetch the state a previous VM
#                          synced off. It sees a staging directory as OUT; the
#                          build is checked there before anything reaches the real OUT
#   --sync-cmd CMD         run after every slice, on SIGTERM/SIGINT and at the end,
#                          to copy OUT somewhere that outlives the VM. A failure
#                          is reported and retried at the next slice
#   --finish-cmd CMD       run once, only after the ledger is written AND a final
#                          sync succeeded (for example shutting the VM down)
# The commands run with `bash -c` and see OUT in the environment. A restored
# directory from a different build (commit or build identity) is refused, because
# a changed identity invalidates the run. A slice cut short by SIGTERM counts the
# time libFuzzer reported; one cut short by the VM vanishing is lost, so shorter
# slices lose less.
#
# Limits: libFuzzer timeout 2 s (a hang). Memory over 512 MiB is the worker's
# own cap (the capped allocator aborts the target), and the libFuzzer RSS limit
# is only a runaway guard because ASan and the corpus inflate RSS well past the
# worker's real use.
set -euo pipefail

NIGHTLY="${NIGHTLY:-nightly}"
OUT=""; WORKERS=2; SLICE=3600; SLICES=1; FORMATS="png,jpeg,webp"; SEEDS=""
IMAGE_PATH_BUILD=""
RSS_MB=2048
MIRI_RECORD=""
TARGET_H=""; RESTORE_CMD=""; SYNC_CMD=""; FINISH_CMD=""
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
    --target-cpu-hours) TARGET_H="$2"; shift 2 ;;
    --restore-cmd) RESTORE_CMD="$2"; shift 2 ;;
    --sync-cmd) SYNC_CMD="$2"; shift 2 ;;
    --finish-cmd) FINISH_CMD="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
export OUT
TOOLS=(cargo run -q --locked -p lumen-decoder-tools --)
cd "$ROOT"

# One campaign per directory.
mkdir "$OUT/.lock" 2>/dev/null || { echo "another run holds $OUT/.lock (remove it if that run is dead)" >&2; exit 5; }

sync_out() { # copy OUT off the VM; never fatal by itself
  [ -n "$SYNC_CMD" ] || return 0
  bash -c "$SYNC_CMD" || { echo "sync failed: $SYNC_CMD" >&2; return 1; }
}
CHILD=""; CUR_TARGET=""
on_signal() {
  echo "signal received: stopping the fuzzer, recording the slice, syncing" >&2
  INTERRUPTED=1
  [ -z "$CHILD" ] || kill -TERM "$CHILD" 2>/dev/null || true
  # cargo does not pass the signal on to the fuzz processes it started.
  [ -z "$CUR_TARGET" ] || pkill -TERM -f "/release/$CUR_TARGET" 2>/dev/null || true
}
INTERRUPTED=0
trap on_signal TERM INT
trap 'rmdir "$OUT/.lock" 2>/dev/null || true' EXIT


[ -n "$SEEDS" ] || { SEEDS="$OUT/seeds"; cargo run -q --locked -p lumen-decoder-tools --example seeds -- "$SEEDS"; }
# The upstream projects' regression inputs, pinned in their manifest. They are
# not CVE reproducers: the chosen libraries have no registered advisories.
REGRESSION="$ROOT/crates/lumen-decoder/fuzz/seeds-regression"

# Identity of what is fuzzed, recorded once per call so a changed build shows.
FUZZ_FLAGS="cargo-fuzz asan; overflow-checks+debug-assertions (no UBSan for Rust); libfuzzer -timeout=2 -rss_limit_mb=$RSS_MB; nightly=$NIGHTLY"
DECODER_BUILD="$("${TOOLS[@]}" identity "$ROOT" "$FUZZ_FLAGS")"
# The image path's identity covers its host half only (runner, process control,
# unlock rule, wire format, limits): the frame parser, reassembly and upload do
# not exist yet. An identity passed with --image-path-build overrides it.
[ -n "$IMAGE_PATH_BUILD" ] || IMAGE_PATH_BUILD="$("${TOOLS[@]}" identity "$ROOT" "$FUZZ_FLAGS" image-path)"
HEAD_COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
# A resumed run must be of this same build, or its CPU-hours do not count for it.
same_build() { # directory -> 0 if it holds no run, or a run of this build
  [ -f "$1/decoder_build" ] || return 0
  [ "$(cat "$1/decoder_build")" = "$DECODER_BUILD" ] \
    && [ "$(cat "$1/image_path_build" 2>/dev/null)" = "$IMAGE_PATH_BUILD" ] \
    && [ "$(cat "$1/commit" 2>/dev/null)" = "$HEAD_COMMIT" ]
}
refuse_msg() { # directory
  echo "refusing to resume: $1 holds a run of a different build (commit $(cat "$1/commit" 2>/dev/null), identities $(cat "$1/decoder_build") and $(cat "$1/image_path_build" 2>/dev/null); this is $HEAD_COMMIT, $DECODER_BUILD and $IMAGE_PATH_BUILD)" >&2
}
refuse() { refuse_msg "$1"; exit 3; }
same_build "$OUT" || refuse "$OUT"

# Restore into a staging directory first and look at it before it touches OUT:
# a refused resume must leave OUT as it was. The restore command sees the staging
# directory as OUT.
if [ -n "$RESTORE_CMD" ]; then
  STAGE="$(mktemp -d "$OUT.restore.XXXXXX")"
  echo "restoring previous state into $STAGE: $RESTORE_CMD" >&2
  OUT="$STAGE" bash -c "$RESTORE_CMD" || { echo "restore failed" >&2; rm -rf "$STAGE"; exit 6; }
  same_build "$STAGE" || { refuse_msg "$STAGE"; rm -rf "$STAGE"; exit 3; }
  rm -rf "$STAGE/.lock"                 # a lock synced from the old VM is not this run's
  cp -a "$STAGE"/. "$OUT"/
  rm -rf "$STAGE"
fi
echo "$DECODER_BUILD" > "$OUT/decoder_build"
echo "$IMAGE_PATH_BUILD" > "$OUT/image_path_build"
echo "$HEAD_COMMIT" > "$OUT/commit"

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

cpu_hours_of() { # samples file -> highest at_cpu_hour
  python3 - "$1" <<'PY'
import json,sys
h=0.0
for l in open(sys.argv[1]):
    if l.strip(): h=max(h,json.loads(l)["at_cpu_hour"])
print(h)
PY
}

ARTIFACT_POLL="${ARTIFACT_POLL:-30}"
# Copies a crash input under a temporary name, checks the copy, then renames it,
# so a copy that is killed half way (or taken while the fuzzer is still writing)
# never leaves a truncated input under its real name, where a later run would
# skip it. Does nothing if the destination exists.
copy_input() { # source, destination
  local src="$1" dst="$2" tmp
  [ -e "$dst" ] && return 0
  tmp="$(dirname "$dst")/.$(basename "$dst").tmp"
  if cp "$src" "$tmp" && cmp -s "$src" "$tmp"; then mv "$tmp" "$dst"; else rm -f "$tmp"; return 1; fi
}
# Copies crash inputs the fuzzer has written into OUT, and syncs when there are
# new ones, so a VM that vanishes mid-slice loses only the last few seconds.
copy_new_artifacts() { # format, target
  local art="$ROOT/crates/lumen-decoder/fuzz/artifacts/$2" dir="$OUT/$1" a new=0
  [ -d "$art" ] || return 0
  mkdir -p "$dir/artifacts"
  rm -f "$dir"/artifacts/.*.tmp        # a copy a previous run did not finish
  for a in $(ls "$art"); do
    [ -e "$dir/artifacts/$a" ] && continue
    copy_input "$art/$a" "$dir/artifacts/$a" && new=1
  done
  [ "$new" = 0 ] || sync_out || true
}

# Records the crashes in artifacts that are not recorded yet, at CPU-hour $3.
record_artifacts() { # format, target, cpu-hour
  local f="$1" target="$2" now_h="$3" art dir seen a k
  art="$ROOT/crates/lumen-decoder/fuzz/artifacts/$target"; dir="$OUT/$f"
  mkdir -p "$dir/artifacts"; touch "$dir/artifacts-seen.txt"
  # Inputs a previous VM synced off but did not get to record.
  rm -f "$dir"/artifacts/.*.tmp        # a copy a previous run did not finish
  mkdir -p "$art"
  for a in $(ls "$dir/artifacts" 2>/dev/null); do copy_input "$dir/artifacts/$a" "$art/$a" || true; done
  for a in $(ls "$art" | sort); do
    grep -qxF "$a" "$dir/artifacts-seen.txt" && continue
    copy_input "$art/$a" "$dir/artifacts/$a" || true   # kept in OUT so a sync carries it off the VM
    k="$(kind_of "$art/$a" "$target")"
    echo "$a" >> "$dir/artifacts-seen.txt"
    if [ "$k" = "asan-artifact" ]; then
      echo "oom report in $f ($a) replayed under the limit; recorded in replays.jsonl, not a crash" >&2
      continue
    fi
    echo "{\"at_cpu_hour\": $now_h, \"kind\": \"${k:-other}\", \"artifact\": \"$a\"}" >> "$dir/crashes.jsonl"
    echo "crash in $f at ${now_h} CPU-hours: ${k:-other} ($a)" >&2
  done
}

run_slice() { # format -> 0 normal, 1 interrupted
  local f="$1" target="decode_$1" dir="$OUT/$1" done_h edges used now_h watch
  # A signal that arrived between slices stops here, before a slice is started.
  if [ "$INTERRUPTED" = 1 ]; then sync_out || true; return 1; fi
  done_h="$(cpu_hours_of "$dir/samples.jsonl")"
  # Crashes a previous VM found but did not get to record.
  record_artifacts "$f" "$target" "$done_h"
  (cd "$ROOT/crates/lumen-decoder" && exec cargo +"$NIGHTLY" fuzz run "$target" "$dir/corpus" -- \
      -fork="$WORKERS" -ignore_crashes=1 -max_total_time="$SLICE" -timeout=2 \
      -rss_limit_mb="$RSS_MB" -max_len=65536 > "$dir/last.log" 2>&1) &
  CHILD=$!; CUR_TARGET="$target"
  # A signal in the moment before CHILD was set found nothing to stop.
  if [ "$INTERRUPTED" = 1 ]; then kill -TERM "$CHILD" 2>/dev/null || true; pkill -TERM -f "/release/$CUR_TARGET" 2>/dev/null || true; fi
  ( while kill -0 "$CHILD" 2>/dev/null; do sleep "$ARTIFACT_POLL"; copy_new_artifacts "$f" "$target"; done ) &
  watch=$!
  wait "$CHILD" || true
  # A signal makes `wait` return early; collect the fuzzer before reading its log.
  wait "$CHILD" 2>/dev/null || true
  # Give the fuzz processes a moment to exit and flush their log.
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -f "/release/$CUR_TARGET" >/dev/null 2>&1 || break; sleep 1; done
  kill "$watch" 2>/dev/null || true; wait "$watch" 2>/dev/null || true
  CHILD=""; CUR_TARGET=""
  if [ "$INTERRUPTED" = 1 ]; then
    # Count the time libFuzzer says it ran, not the full slice.
    used="$(grep -oE 'time: [0-9]+s' "$dir/last.log" | tail -1 | grep -oE '[0-9]+' || echo 0)"
    used="${used:-0}"
  else
    used="$SLICE"
  fi
  edges="$(grep -E '^#[0-9]+: cov:' "$dir/last.log" | tail -1 | sed -E 's/.*cov: ([0-9]+).*/\1/' || true)"
  now_h="$(python3 -c "print($done_h + $used * $WORKERS / 3600.0)")"
  if [ -n "${edges:-}" ] && [ "$used" -gt 0 ]; then
    echo "{\"at_cpu_hour\": $now_h, \"edges\": $edges}" >> "$dir/samples.jsonl"
  else
    echo "no coverage line (or no time) in $dir/last.log; slice not counted" >&2
  fi
  record_artifacts "$f" "$target" "$now_h"
  sync_out || true
  [ "$INTERRUPTED" = 1 ] && return 1
  return 0
}

for f in "${FMTS[@]}"; do
  dir="$OUT/$f"; mkdir -p "$dir/corpus"
  cp -n "$SEEDS/$f"/* "$dir/corpus/" 2>/dev/null || true
  cp -n "$REGRESSION/$f"/* "$dir/corpus/" 2>/dev/null || true
  touch "$dir/samples.jsonl" "$dir/crashes.jsonl"
  if [ -n "$TARGET_H" ]; then
    while python3 -c "import sys; sys.exit(0 if $(cpu_hours_of "$dir/samples.jsonl") < $TARGET_H else 1)"; do
      run_slice "$f" || { echo "interrupted; state synced" >&2; exit 143; }
    done
  else
    for _ in $(seq 1 "$SLICES"); do
      run_slice "$f" || { echo "interrupted; state synced" >&2; exit 143; }
    done
  fi
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
cat > "$OUT/image_path.json" <<'JSON'
{"scope": "partial", "covers": "the host half of the image path: the runner (bounds and validates a worker's reply), process control, the unlock rule, the wire format and the limits", "does_not_cover": "the image-frame parser, multi-frame reassembly and the upload step, which do not exist yet, so the stateful multi-frame fuzz targets of the plan cannot be run", "reading": "a pass on this ledger is not a pass of the whole image path"}
JSON
NOTES=("image_path=$OUT/image_path.json" "past_cve_seeds=$ROOT/crates/lumen-decoder/fuzz/seeds-regression/PAST_CVES.json" "build=$OUT/build.json" "ubsan=$OUT/ubsan.json")
[ -n "$MIRI_RECORD" ] || MIRI_RECORD="$ROOT/crates/lumen-decoder/fuzz/miri-record.json"
[ ! -f "$MIRI_RECORD" ] || NOTES+=("miri=$MIRI_RECORD" "miri_gaps=$ROOT/crates/lumen-decoder/fuzz/miri-gaps.json")
"${TOOLS[@]}" annotate "$OUT/ledger.plain.json" "${NOTES[@]}" > "$OUT/ledger.json"
echo "ledger: $OUT/ledger.json"

# Final sync, retried; the finish command (for example shutting the VM down)
# runs only when the ledger has been carried off.
ok=0
for attempt in 1 2 3; do
  if sync_out; then ok=1; break; fi
  sleep 30
done
if [ -n "$FINISH_CMD" ]; then
  if [ "$ok" = 1 ]; then
    echo "finished and synced; running the finish command" >&2
    bash -c "$FINISH_CMD"
  else
    echo "final sync failed three times: NOT running the finish command, so the VM keeps its results. Fix the sync and run it by hand." >&2
    exit 4
  fi
fi
