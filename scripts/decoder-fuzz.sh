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
# The gate run (--gate-run), with the stop rule of the campaign plan:
#   --gate-run             run every format in round-robin slices; count measured
#                          CPU time (not workers x wall time); stop a format when
#                          the gate holds for it (at least 250 CPU-hours, no crash
#                          at all, coverage flat over the last 168, by the judge's
#                          rule), or at --cap-cpu-hours; write status.json
#   --cap-cpu-hours H      cap per format (default 1200). The last slice is sized
#                          in crates/lumen-decoder-tools/src/slice.rs from what is
#                          left of the cap, assuming the worst of two things: a
#                          startup of 30 CPU-seconds per slice, and a rate of 1.25
#                          times the highest CPU per worker-second any earlier
#                          slice showed (the first slice takes at most 40% of what
#                          is left). While both hold the counted hours never pass
#                          the cap, and normally end a few percent under it; when
#                          one breaks, the cap is passed by about the excess.
#   --max-wall-hours W     hard stop for the whole call, exit 8
#   --pause-on-crash-after H  a crash recorded after H CPU-hours of its format
#                          pauses the run (exit 7, state synced, the finish command
#                          is NOT run): default 0, any crash. A crash ends the run
#                          for that build identity, which cannot pass however many
#                          clean hours follow; the fix changes the build identity
#                          and the gate runs again from zero. A larger H lets the
#                          run go on after an early crash, for a run that only
#                          wants the data.
#   --price-per-hour P     for the estimated cost in status.json
#   --stats-dir DIR        also write DIR/<fmt>.env (EXECS_PER_SEC COVERAGE
#                          CORPUS_SIZE CRASHES LAST_CRASH_EPOCH LAST_COV_EPOCH, plus
#                          CPU_HOURS and STATE) after every slice, for a metrics push
#   --console PATH         append one "FUZZ-STATUS key=value ..." line per format
#                          and slice (and one for the whole run) to PATH, normally
#                          /dev/console, so a launcher can read it from the serial
#                          console with no provider access from the VM
#   --console-prefix WORD  the first word of those lines (default FUZZ-STATUS)
#   A format whose slices count nothing three times running (the fuzzer cannot
#   start, or prints no coverage) stops the run with exit 9 instead of spinning.
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
#   --checkpoint-console PATH
#                          for a VM that can only write to its serial console: every
#                          --checkpoint-minutes (default 60), and at the end, write one
#                          self-contained block (CKPT lines, see ckpt.rs) with each
#                          format's samples, crashes, state and the build identity, and
#                          the crash inputs if they fit. THE CORPUS IS NOT CARRIED: a
#                          run restored from a block starts from the seeds, so its
#                          coverage may dip until the fuzzer finds its way back, while
#                          its CPU-hours and crash records carry over. A block holds at
#                          most about 21 KB; when the state is bigger the crash inputs
#                          are left out, and then the samples and crashes alone
#   --restore-console FILE restore from the newest verified block in FILE (console text),
#                          through the same staging directory and build check as
#                          --restore-cmd
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
OUT=""; WORKERS=2; SLICE=3600; SLICE_NOW=3600; SLICES=1; FORMATS="png,jpeg,webp"; SEEDS=""
IMAGE_PATH_BUILD=""
RSS_MB=2048
MIRI_RECORD=""
TARGET_H=""; RESTORE_CMD=""; SYNC_CMD=""; FINISH_CMD=""
GATE_RUN=0; CAP_H=1200; MAX_WALL_H=""; PAUSE_AFTER=0; PRICE=""; STATS_DIR=""; CONSOLE=""; CONSOLE_PREFIX="FUZZ-STATUS"; CKPT_PATH=""; CKPT_MIN=60; RESTORE_CONSOLE=""
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
    --gate-run) GATE_RUN=1; shift ;;
    --cap-cpu-hours) CAP_H="$2"; shift 2 ;;
    --max-wall-hours) MAX_WALL_H="$2"; shift 2 ;;
    --pause-on-crash-after) PAUSE_AFTER="$2"; shift 2 ;;
    --price-per-hour) PRICE="$2"; shift 2 ;;
    --stats-dir) STATS_DIR="$2"; shift 2 ;;
    --console) CONSOLE="$2"; shift 2 ;;
    --console-prefix) CONSOLE_PREFIX="$2"; shift 2 ;;
    --checkpoint-console) CKPT_PATH="$2"; shift 2 ;;
    --checkpoint-minutes) CKPT_MIN="$2"; shift 2 ;;
    --restore-console) RESTORE_CONSOLE="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 2; }
SLICE_NOW="$SLICE"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
export OUT
TOOLS=(cargo run -q --locked -p lumen-decoder-tools --)
cd "$ROOT"

# One campaign per directory.
mkdir "$OUT/.lock" 2>/dev/null || { echo "another run holds $OUT/.lock (remove it if that run is dead)" >&2; exit 5; }

LAST_CKPT=0
# One console checkpoint block: the small state files, never the corpus. Not
# fatal: a block that does not fit is reported and the run goes on.
checkpoint_out() { # [force]
  [ -n "$CKPT_PATH" ] || return 0
  local now; now="$(date +%s)"
  [ "${1:-}" = force ] || [ $(( now - LAST_CKPT )) -ge $(( CKPT_MIN * 60 )) ] || return 0
  local seq=$(( $(cat "$OUT/ckpt_seq" 2>/dev/null || echo 0) + 1 )) kind=hourly tmp files f fmt rc=1
  [ "${1:-}" != force ] || kind=final
  echo "$seq" > "$OUT/ckpt_seq"
  tmp="$(mktemp -d)"
  for with in artifacts none; do
    files=()
    for f in commit decoder_build image_path_build ckpt_seq slices.log status.json ledger.json build.json; do [ ! -e "$OUT/$f" ] || files+=("$f"); done
    for fmt in ${FORMATS//,/ }; do
      for f in samples.jsonl crashes.jsonl replays.jsonl state noprogress nominal.txt gate.json ratio artifacts-seen.txt; do
        [ ! -e "$OUT/$fmt/$f" ] || files+=("$fmt/$f")
      done
      [ "$with" != artifacts ] || [ ! -d "$OUT/$fmt/artifacts" ] || files+=("$fmt/artifacts")
    done
    tar czf "$tmp/state.tgz" -C "$OUT" "${files[@]}" 2>/dev/null || continue
    if "${TOOLS[@]}" ckpt-encode "$CONSOLE_PREFIX" "$(printf '%.8s' "$HEAD_COMMIT")-$seq" "$seq" "$kind" "$tmp/state.tgz" > "$tmp/block.txt" 2>"$tmp/err.txt"; then
      cat "$tmp/block.txt" >> "$CKPT_PATH" 2>/dev/null || cat "$tmp/block.txt"
      LAST_CKPT="$now"; rc=0
      echo "console checkpoint $seq ($kind, $with) written" >&2
      break
    fi
  done
  [ "$rc" = 0 ] || echo "console checkpoint $seq did not fit: $(cat "$tmp/err.txt")" >&2
  rm -rf "$tmp"
  return 0
}
sync_out() { # copy OUT off the VM; never fatal by itself
  checkpoint_out "${1:-}"
  [ -n "$SYNC_CMD" ] || return 0
  bash -c "$SYNC_CMD" || { echo "sync failed: $SYNC_CMD" >&2; return 1; }
}
CHILD=""; CUR_TARGET=""; CUR_RAW=""
on_signal() {
  echo "signal received: stopping the fuzzer, recording the slice, syncing" >&2
  INTERRUPTED=1
  [ -z "$CHILD" ] || kill -TERM "$CHILD" 2>/dev/null || true
  # cargo does not pass the signal on to the fuzz processes it started.
  # Only this run's fuzz processes: their command line carries this run's
  # directory (-artifact_prefix), so a concurrent run is not touched.
  [ -z "$CUR_RAW" ] || pkill -TERM -f -- "-artifact_prefix=$CUR_RAW/" 2>/dev/null || true
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
if [ -n "$RESTORE_CONSOLE" ]; then
  [ -z "$RESTORE_CMD" ] || { echo "--restore-console and --restore-cmd are alternatives" >&2; exit 2; }
  RESTORE_CMD='f="$(mktemp)" && cargo run -q --locked -p lumen-decoder-tools -- ckpt-decode "'"$RESTORE_CONSOLE"'" "$f" && tar xzf "$f" -C "$OUT" --no-same-owner; rc=$?; rm -f "$f"; exit $rc'
fi
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
  local art="$OUT/$1/raw-artifacts" dir="$OUT/$1" a new=0
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
  art="$OUT/$f/raw-artifacts"; dir="$OUT/$f"
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
    echo "{\"at_cpu_hour\": $now_h, \"kind\": \"${k:-other}\", \"artifact\": \"$a\", \"at_epoch\": $(date +%s)}" >> "$dir/crashes.jsonl"
    echo "crash in $f at ${now_h} CPU-hours: ${k:-other} ($a)" >&2
  done
}

# How long the next slice of a format may run: the slice length, cut to what is
# left of the CPU cap (assuming every worker is busy, so the measured time cannot
# pass the cap by more than a rounding) and of the wall-clock limit. Prints the
# seconds; 0 means the cap or the limit is reached.
slice_for() { # format -> seconds
  local rate="-" wall="-"
  if [ "$GATE_RUN" != 1 ]; then echo "$SLICE"; return; fi
  [ ! -s "$OUT/$1/ratio" ] || rate="$(cat "$OUT/$1/ratio")"
  [ -z "${MAX_WALL_H:-}" ] || wall="$(python3 -c "import sys,time; print(max(0.0, float(sys.argv[1])*3600.0-(time.time()-int(sys.argv[2]))))" "$MAX_WALL_H" "$START_EPOCH")"
  "${TOOLS[@]}" slice-seconds "$SLICE" "$WORKERS" "$CAP_H" "$(hours_of "$1" cpu_hours)" "$rate" "$wall"
}

run_slice() { # format -> 0 normal, 1 interrupted
  local f="$1" target="decode_$1" dir="$OUT/$1" done_h edges used now_h watch
  # A signal that arrived between slices stops here, before a slice is started.
  if [ "$INTERRUPTED" = 1 ]; then sync_out || true; return 1; fi
  done_h="$(cpu_hours_of "$dir/samples.jsonl")"
  # Crashes a previous VM found but did not get to record.
  record_artifacts "$f" "$target" "$done_h"
  mkdir -p "$dir/raw-artifacts"
  rm -f "$dir/cpu_seconds"
  # Build the target first and outside the CPU measurement: compiling it is not
  # fuzzing, and on a fresh VM it takes minutes of CPU. (A dry run on a 2-vCPU
  # instance counted the compile as fuzzing time and capped a format after one
  # slice.) When it is already built this is a no-op; if it fails, the run below
  # fails the same way and the no-progress stop applies.
  (cd "$ROOT/crates/lumen-decoder" && cargo +"$NIGHTLY" fuzz build "$target" > "$dir/build.log" 2>&1) || true
  local runner=()
  [ "$GATE_RUN" = 1 ] && runner=(python3 "$ROOT/scripts/decoder-cpu-time.py" "$dir/cpu_seconds")
  (cd "$ROOT/crates/lumen-decoder" && exec ${runner[@]+"${runner[@]}"} cargo +"$NIGHTLY" fuzz run "$target" "$dir/corpus" -- \
      -fork="$WORKERS" -ignore_crashes=1 -max_total_time="$SLICE_NOW" -timeout=2 \
      -artifact_prefix="$dir/raw-artifacts/" \
      -rss_limit_mb="$RSS_MB" -max_len=65536 > "$dir/last.log" 2>&1) &
  CHILD=$!; CUR_TARGET="$target"; CUR_RAW="$dir/raw-artifacts"
  local t0; t0="$(python3 -c 'import time; print(time.time())')"
  # A signal in the moment before CHILD was set found nothing to stop.
  if [ "$INTERRUPTED" = 1 ]; then kill -TERM "$CHILD" 2>/dev/null || true; pkill -TERM -f -- "-artifact_prefix=$CUR_RAW/" 2>/dev/null || true; fi
  ( while kill -0 "$CHILD" 2>/dev/null; do sleep "$ARTIFACT_POLL"; copy_new_artifacts "$f" "$target"; done ) &
  watch=$!
  wait "$CHILD" || true
  # A signal makes `wait` return early; collect the fuzzer before reading its log.
  wait "$CHILD" 2>/dev/null || true
  # Give the fuzz processes a moment to exit and flush their log.
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -f -- "-artifact_prefix=$CUR_RAW/" >/dev/null 2>&1 || break; sleep 1; done
  kill "$watch" 2>/dev/null || true; wait "$watch" 2>/dev/null || true
  CHILD=""; CUR_TARGET=""; CUR_RAW=""
  # Learn how much CPU a worker-second really costs here: the highest ratio seen.
  if [ "$GATE_RUN" = 1 ] && [ -s "$dir/cpu_seconds" ]; then
    local prev="-" wall_s
    [ ! -s "$dir/ratio" ] || prev="$(cat "$dir/ratio")"
    wall_s="$(python3 -c "import sys,time; print(max(1.0, time.time()-float(sys.argv[1])))" "$t0")"
    "${TOOLS[@]}" learn-rate "$prev" "$(cat "$dir/cpu_seconds")" "$WORKERS" "$wall_s" > "$dir/ratio.new" && mv "$dir/ratio.new" "$dir/ratio"
  fi
  if [ "$INTERRUPTED" = 1 ]; then
    # Count the time libFuzzer says it ran, not the full slice.
    used="$(grep -oE 'time: [0-9]+s' "$dir/last.log" | tail -1 | grep -oE '[0-9]+' || echo 0)"
    used="${used:-0}"
  else
    used="$SLICE_NOW"
  fi
  edges="$(grep -E '^#[0-9]+: cov:' "$dir/last.log" | tail -1 | sed -E 's/.*cov: ([0-9]+).*/\1/' || true)"
  now_h="$(python3 -c "print($done_h + $used * $WORKERS / 3600.0)")"
  # In the gate run the hours are the CPU time the fuzz processes really used.
  if [ "$GATE_RUN" = 1 ] && [ -s "$dir/cpu_seconds" ]; then
    echo "$used $WORKERS" >> "$dir/nominal.txt"
    now_h="$(python3 -c "print($done_h + $(cat "$dir/cpu_seconds") / 3600.0)")"
    used=1
  elif [ "$GATE_RUN" = 1 ]; then
    used=0
  fi
  local np; np="$(cat "$dir/noprogress" 2>/dev/null || echo 0)"
  if [ -n "${edges:-}" ] && [ "$used" -gt 0 ]; then
    echo 0 > "$dir/noprogress"
    echo "{\"at_cpu_hour\": $now_h, \"edges\": $edges, \"at_epoch\": $(date +%s)}" >> "$dir/samples.jsonl"
  else
    echo "no coverage line (or no time) in $dir/last.log; slice not counted" >&2
    echo $((np + 1)) > "$dir/noprogress"
  fi
  record_artifacts "$f" "$target" "$now_h"
  sync_out || true
  [ "$INTERRUPTED" = 1 ] && return 1
  return 0
}

START_EPOCH="$(date +%s)"; PAUSED=""; PAUSE_CODE=0

# status.json: the numbers a dashboard needs, one object per format.
write_status() { # overall state word
  local f dir
  for f in "${FMTS[@]}"; do
    dir="$OUT/$f"
    "${TOOLS[@]}" gate-status "$dir/samples.jsonl" "$dir/crashes.jsonl" ${LUMEN_GATE_MIN_HOURS:+"$LUMEN_GATE_MIN_HOURS" "${LUMEN_GATE_WINDOW_HOURS:?}"} > "$dir/gate.json" 2>/dev/null || echo '{}' > "$dir/gate.json"
  done
  python3 - "$OUT" "$FORMATS" "$1" "$START_EPOCH" "${PRICE:-}" "${STATS_DIR:-}" "${CONSOLE:-}" "${CONSOLE_PREFIX:-FUZZ-STATUS}" <<'PY'
import json, os, re, sys, time
out, formats, overall, start, price, stats_dir, console, prefix = sys.argv[1:9]
now = int(time.time())
elapsed = (time.time() - int(start)) / 3600.0
doc = {"updated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "state": overall,
       "elapsed_wall_hours_this_call": round(elapsed, 4),
       "estimated_cost_usd": round(elapsed * float(price), 2) if price else None, "formats": {}}

def lines(path):
    out = []
    try:
        for l in open(path):
            if l.strip():
                out.append(json.loads(l))
    except (OSError, ValueError):
        pass
    return out

def epochs(d):
    """(first sample epoch, last crash epoch, epoch of the last coverage gain)."""
    samples = sorted(lines(os.path.join(d, "samples.jsonl")), key=lambda r: r.get("at_cpu_hour", 0))
    crashes = lines(os.path.join(d, "crashes.jsonl"))
    first = next((r["at_epoch"] for r in samples if "at_epoch" in r), None)
    best, gain = -1, None
    for r in samples:
        if r.get("edges", 0) > best:
            best = r.get("edges", 0)
            gain = r.get("at_epoch", gain)
    last_crash = max((c["at_epoch"] for c in crashes if "at_epoch" in c), default=None)
    return first, last_crash, gain

console_lines = []
for f in formats.split(","):
    d = os.path.join(out, f)
    try:
        g = json.load(open(os.path.join(d, "gate.json")))
    except Exception:
        g = {}
    log = ""
    try:
        log = open(os.path.join(d, "last.log"), errors="replace").read().replace("\r", "\n")
    except OSError:
        pass
    ls = [l for l in log.splitlines() if re.match(r"^#\d+: cov:", l)]
    last = ls[-1] if ls else ""
    execs = re.search(r"exec/s: (\d+)", last)
    corp = re.search(r"corp: (\d+)", last)
    state = "running"
    try:
        state = open(os.path.join(d, "state")).read().strip() or "running"
    except OSError:
        pass
    nominal = 0.0
    try:
        for l in open(os.path.join(d, "nominal.txt")):
            a, b = l.split()
            nominal += float(a) * float(b) / 3600.0
    except OSError:
        pass
    corpus_n = int(corp.group(1)) if corp else (len(os.listdir(os.path.join(d, "corpus"))) if os.path.isdir(os.path.join(d, "corpus")) else 0)
    first, last_crash, gain = epochs(d)
    # With no crash yet the "last crash" is the start of the campaign, so the age
    # reads as the time run so far and not as zero.
    last_crash_epoch = last_crash or first or int(start)
    last_cov_epoch = gain or first or int(start)
    doc["formats"][f] = {
        "state": state,
        "CpuHours": g.get("cpu_hours", 0.0),
        "NominalWorkerHours": round(nominal, 4),
        "ExecsPerSec": int(execs.group(1)) if execs else 0,
        "Coverage": g.get("edges", 0),
        "CorpusSize": corpus_n,
        "Crashes": g.get("crashes", 0),
        "HoursSinceNewCrash": g.get("hours_since_new_crash", 0.0),
        "HoursSinceCoverageGain": g.get("hours_since_coverage_gain", 0.0),
        "GateHolds": g.get("holds", False),
        "LastCrashEpoch": last_crash_epoch,
        "LastCoverageEpoch": last_cov_epoch,
    }
    kv = [("EXECS_PER_SEC", doc["formats"][f]["ExecsPerSec"]), ("COVERAGE", int(doc["formats"][f]["Coverage"])),
          ("CORPUS_SIZE", corpus_n), ("CRASHES", doc["formats"][f]["Crashes"]),
          ("LAST_CRASH_EPOCH", last_crash_epoch), ("LAST_COV_EPOCH", last_cov_epoch),
          ("CPU_HOURS", round(doc["formats"][f]["CpuHours"], 4)), ("STATE", state.replace(" ", "_"))]
    if stats_dir:
        os.makedirs(stats_dir, exist_ok=True)
        tmp = os.path.join(stats_dir, "." + f + ".env.tmp")
        with open(tmp, "w") as fh:
            fh.write("".join(f"{k}={v}\n" for k, v in kv))
        os.replace(tmp, os.path.join(stats_dir, f + ".env"))
    console_lines.append(prefix + " fmt=" + f + " " + " ".join(f"{k}={v}" for k, v in kv) + f" EPOCH={now}")
cost = doc["estimated_cost_usd"]
console_lines.append(prefix + " overall state=" + overall.replace(" ", "_") + f" ELAPSED_H={doc['elapsed_wall_hours_this_call']}" + (f" COST_USD={cost}" if cost is not None else "") + f" EPOCH={now}")
if console:
    try:
        with open(console, "a") as fh:
            fh.write("\n".join(console_lines) + "\n")
    except OSError:
        pass
tmp = os.path.join(out, ".status.json.tmp")
json.dump(doc, open(tmp, "w"), indent=2)
os.replace(tmp, os.path.join(out, "status.json"))
PY
}

set_state() { echo "$2" > "$OUT/$1/state"; }
# Three slices in a row that counted nothing mean the fuzzer is not running (no
# nightly, no cargo-fuzz, a build that fails): stop instead of looping forever.
no_progress_guard() { # format
  local n; n="$(cat "$OUT/$1/noprogress" 2>/dev/null || echo 0)"
  [ "$n" -lt 3 ] && return 0
  echo "no progress: 3 slices of $1 counted nothing (see $OUT/$1/last.log); stopping" >&2
  [ "$GATE_RUN" = 1 ] && write_status "no progress in $1"
  sync_out || true
  exit 9
}
get_state() { cat "$OUT/$1/state" 2>/dev/null || echo running; }
hours_of() { "${TOOLS[@]}" gate-status "$OUT/$1/samples.jsonl" "$OUT/$1/crashes.jsonl" ${LUMEN_GATE_MIN_HOURS:+"$LUMEN_GATE_MIN_HOURS" "${LUMEN_GATE_WINDOW_HOURS:?}"} | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['$2'])"; }

for f in "${FMTS[@]}"; do
  dir="$OUT/$f"; mkdir -p "$dir/corpus"
  cp -n "$SEEDS/$f"/* "$dir/corpus/" 2>/dev/null || true
  cp -n "$REGRESSION/$f"/* "$dir/corpus/" 2>/dev/null || true
  touch "$dir/samples.jsonl" "$dir/crashes.jsonl"
done

if [ "$GATE_RUN" = 1 ]; then
  while [ -z "$PAUSED" ]; do
    active=0
    for f in "${FMTS[@]}"; do
      [ "$(get_state "$f")" = running ] || continue
      if [ "$(hours_of "$f" holds)" = True ]; then set_state "$f" done; continue; fi
      if python3 -c "import sys; sys.exit(0 if $(hours_of "$f" cpu_hours) >= $CAP_H else 1)"; then set_state "$f" capped; continue; fi
      SLICE_NOW="$(slice_for "$f")"
      if [ "$SLICE_NOW" -le 0 ]; then
        if [ -n "$MAX_WALL_H" ] && python3 -c "import sys,time; sys.exit(0 if (time.time()-$START_EPOCH)/3600.0 >= $MAX_WALL_H - 5/3600.0 else 1)"; then
          PAUSED="wall-clock limit of $MAX_WALL_H hours"; PAUSE_CODE=8; break
        fi
        set_state "$f" capped; continue
      fi
      active=1
      crashes_before="$(wc -l < "$OUT/$f/crashes.jsonl" | tr -d ' ')"
      run_slice "$f" || { write_status interrupted; echo "interrupted; state synced" >&2; exit 143; }
      no_progress_guard "$f"
      crashes_after="$(wc -l < "$OUT/$f/crashes.jsonl" | tr -d ' ')"
      now_h="$(hours_of "$f" cpu_hours)"
      echo "$f $now_h" >> "$OUT/slices.log"      # the order slices ran in, for audit
      if [ "$crashes_after" -gt "$crashes_before" ] \
         && python3 -c "import sys; sys.exit(0 if $now_h >= $PAUSE_AFTER else 1)"; then
        PAUSED="a crash in $f at $now_h CPU-hours (pause-on-crash-after $PAUSE_AFTER)"; PAUSE_CODE=7; break
      fi
      if [ -n "$MAX_WALL_H" ] && python3 -c "import sys,time; sys.exit(0 if (time.time()-$START_EPOCH)/3600.0 >= $MAX_WALL_H else 1)"; then
        PAUSED="wall-clock limit of $MAX_WALL_H hours"; PAUSE_CODE=8; break
      fi
      write_status running
      sync_out || true
    done
    [ "$active" = 1 ] || break
  done
  if [ -n "$PAUSED" ]; then echo "PAUSED: $PAUSED" >&2; write_status "paused: $PAUSED"; else write_status finished; fi
fi

for f in "${FMTS[@]}"; do
  [ "$GATE_RUN" = 1 ] && break
  dir="$OUT/$f"
  if [ -n "$TARGET_H" ]; then
    while python3 -c "import sys; sys.exit(0 if $(cpu_hours_of "$dir/samples.jsonl") < $TARGET_H else 1)"; do
      run_slice "$f" || { echo "interrupted; state synced" >&2; exit 143; }
      no_progress_guard "$f"
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
  if sync_out force; then ok=1; break; fi
  sleep 30
done
if [ -n "$PAUSED" ]; then
  echo "paused, so the finish command is NOT run; the VM keeps its state. Reason: $PAUSED" >&2
  exit "$PAUSE_CODE"
fi
if [ -n "$FINISH_CMD" ]; then
  if [ "$ok" = 1 ]; then
    echo "finished and synced; running the finish command" >&2
    bash -c "$FINISH_CMD"
  else
    echo "final sync failed three times: NOT running the finish command, so the VM keeps its results. Fix the sync and run it by hand." >&2
    exit 4
  fi
fi
