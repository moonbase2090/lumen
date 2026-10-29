#!/usr/bin/env bash
# Tests the interruption-safety of scripts/decoder-fuzz.sh with real (short)
# fuzz runs: a sync command, a restore, a SIGTERM mid-slice, a refusal to resume
# a different build, and a finish command that runs only after a good sync.
# Takes a few minutes. Needs the same toolchain as the campaign.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/scripts/decoder-fuzz.sh"
T="$(mktemp -d)"; trap 'pkill -f "/release/decode_png" 2>/dev/null; rm -rf "$T"' EXIT
export NIGHTLY="${NIGHTLY:-nightly}"
COMMON=(--workers 2 --slice-seconds 8 --formats png)
fail=0
check() { # description, command...
  desc="$1"; shift
  if "$@"; then echo "ok    $desc"; else echo "FAIL  $desc"; fail=1; fi
}
hours() { python3 - "$1" <<'PY'
import json,sys
h=0.0
for l in open(sys.argv[1]):
    if l.strip(): h=max(h,json.loads(l)["at_cpu_hour"])
print(h)
PY
}

# 1. A normal run: synced after every slice and at the end, finish runs last.
"$RUN" --out "$T/a" "${COMMON[@]}" --slices 2 \
  --sync-cmd "rsync -a --delete \"\$OUT/\" \"$T/dest-a/\"" \
  --finish-cmd "date > \"$T/finished-a\"" > "$T/a.log" 2>&1
check "a normal run exits 0" test $? -eq 0
check "the synced copy has the ledger" test -s "$T/dest-a/ledger.json"
check "the synced copy has both samples" test "$(wc -l < "$T/dest-a/png/samples.jsonl" | tr -d ' ')" -eq 2
check "the finish command ran" test -f "$T/finished-a"

# 2. A failing sync: the finish command must not run, and the exit says so.
"$RUN" --out "$T/b" "${COMMON[@]}" --slices 1 --sync-cmd "false" --finish-cmd "date > \"$T/finished-b\"" > "$T/b.log" 2>&1
check "a failing sync exits 4" test $? -eq 4
check "the finish command did not run after a failed sync" test ! -e "$T/finished-b"

# 3. SIGTERM mid-slice: the partial slice is recorded, state synced, exit 143.
"$RUN" --out "$T/c" --workers 2 --slice-seconds 120 --formats png --slices 1 \
  --sync-cmd "rsync -a --delete \"\$OUT/\" \"$T/dest-c/\"" > "$T/c.log" 2>&1 &
pid=$!
for _ in $(seq 1 120); do grep -qE '^#[0-9]+: cov:' "$T/c/png/last.log" 2>/dev/null && break; sleep 1; done
sleep 12
kill -TERM "$pid"; wait "$pid"; rc=$?
check "SIGTERM exits 143" test "$rc" -eq 143
check "no fuzz process is left" test -z "$(pgrep -f '/release/decode_png')"
check "the partial slice was recorded in the synced copy" test "$(wc -l < "$T/dest-c/png/samples.jsonl" | tr -d ' ')" -eq 1
h1="$(hours "$T/dest-c/png/samples.jsonl")"
check "the partial slice counts far less than the 120 s slice ($h1 CPU-hours)" python3 -c "import sys; sys.exit(0 if 0 < $h1 < 2*120/3600.0*0.6 else 1)"

# 4. A replacement VM: restore, then run until a target; hours continue from the sync.
"$RUN" --out "$T/d" "${COMMON[@]}" --target-cpu-hours 0.01 \
  --restore-cmd "mkdir -p \"\$OUT\" && rsync -a \"$T/dest-c/\" \"\$OUT/\"" \
  --sync-cmd "rsync -a --delete \"\$OUT/\" \"$T/dest-d/\"" > "$T/d.log" 2>&1
check "the resumed run exits 0" test $? -eq 0
h2="$(hours "$T/dest-d/png/samples.jsonl")"
check "hours continued past the synced $h1 to at least 0.01 ($h2)" python3 -c "import sys; sys.exit(0 if $h2 >= 0.01 and $h2 > $h1 else 1)"
check "the corpus came back with the restore" test "$(ls "$T/d/png/corpus" | wc -l | tr -d ' ')" -gt 40

# 5. A restored run of a different build is refused.
mkdir -p "$T/e" && echo deadbeef > "$T/e/commit" && echo 0000 > "$T/e/decoder_build"
"$RUN" --out "$T/e" "${COMMON[@]}" --slices 1 > "$T/e.log" 2>&1
check "a different build is refused with exit 3" test $? -eq 3
check "nothing was fuzzed for it" test ! -e "$T/e/png/samples.jsonl"

# 6. Two runs cannot share a directory.
mkdir -p "$T/f/.lock"
"$RUN" --out "$T/f" "${COMMON[@]}" --slices 1 > "$T/f.log" 2>&1
check "a held lock exits 5" test $? -eq 5

[ "$fail" -eq 0 ] && echo "all spot-VM checks passed" || { echo "some checks FAILED"; tail -5 "$T"/*.log; }
exit "$fail"
