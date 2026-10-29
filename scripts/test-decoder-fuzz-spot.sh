#!/usr/bin/env bash
# Tests the interruption-safety of scripts/decoder-fuzz.sh with real (short)
# fuzz runs: a sync command, a restore, a SIGTERM mid-slice, a refusal to resume
# a different build, and a finish command that runs only after a good sync.
# Takes a few minutes. Needs the same toolchain as the campaign.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/scripts/decoder-fuzz.sh"
T="$(mktemp -d)"
ART="$ROOT/crates/lumen-decoder/fuzz/artifacts/decode_png"
# The campaign records every input in the artifacts directory, so the test starts
# from an empty one and puts any existing inputs back afterwards.
if [ -d "$ART" ]; then mv "$ART" "$T/art-backup"; fi
restore_art() { rm -rf "$ART"; if [ -d "$T/art-backup" ]; then mv "$T/art-backup" "$ART"; fi; }
trap 'pkill -f "/release/decode_png" 2>/dev/null; restore_art; rm -rf "$T"' EXIT
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
check "the lock is released at exit" test ! -e "$T/a/.lock"
check "the ledger carries a real image-path identity, not 'absent'" python3 -c "
import json,sys
l=json.load(open('$T/dest-a/ledger.json'))
sys.exit(0 if len(l['image_path_build'])==64 and l['image_path_build']!=l['decoder_build'] else 1)"
check "...and says that it covers the host half only" python3 -c "
import json,sys
l=json.load(open('$T/dest-a/ledger.json'))
sys.exit(0 if l['notes']['image_path']['scope']=='partial' else 1)"

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
check "the lock is held while the run is alive (the slice watcher did not release it)" test -d "$T/c/.lock"
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

# 5. A restore of a different build is refused and leaves OUT as it was.
"$RUN" --out "$T/e" "${COMMON[@]}" --slices 1 \
  --restore-cmd "mkdir -p \"\$OUT\" && echo deadbeef > \"\$OUT/commit\" && echo 0000 > \"\$OUT/decoder_build\" && mkdir -p \"\$OUT/png/corpus\" && echo x > \"\$OUT/png/corpus/foreign\"" > "$T/e.log" 2>&1
check "a restore of a different build is refused with exit 3" test $? -eq 3
check "nothing of it was copied into OUT" test ! -e "$T/e/commit" -a ! -e "$T/e/png/corpus/foreign"
check "no staging directory is left behind" test -z "$(ls -d "$T"/e.restore.* 2>/dev/null)"
check "nothing was fuzzed for it" test ! -e "$T/e/png/samples.jsonl"
# The same refusal for a directory that already holds another build's run.
mkdir -p "$T/e2" && echo deadbeef > "$T/e2/commit" && echo 0000 > "$T/e2/decoder_build"
"$RUN" --out "$T/e2" "${COMMON[@]}" --slices 1 > "$T/e2.log" 2>&1
check "a directory of a different build is refused with exit 3" test $? -eq 3

# 6. Two runs cannot share a directory.
mkdir -p "$T/f/.lock"
"$RUN" --out "$T/f" "${COMMON[@]}" --slices 1 > "$T/f.log" 2>&1
check "a held lock exits 5" test $? -eq 5

# 7. A crash input is copied and synced while the slice is still running.
ARTIFACT_POLL=2 "$RUN" --out "$T/g" --workers 2 --slice-seconds 60 --formats png --slices 1 \
  --sync-cmd "rsync -a --delete \"\$OUT/\" \"$T/dest-g/\"" > "$T/g.log" 2>&1 &
pid=$!
for _ in $(seq 1 120); do grep -qE '^#[0-9]+: cov:' "$T/g/png/last.log" 2>/dev/null && break; sleep 1; done
mkdir -p "$ART"; echo "not a real crash" > "$ART/crash-spot-test"
sleep 8
check "the crash input reached the synced copy before the slice ended" test -e "$T/dest-g/png/artifacts/crash-spot-test"
check "...while the run was still going" kill -0 "$pid"
kill -TERM "$pid"; wait "$pid"
rm -f "$ART/crash-spot-test"

# 8. A signal before the first slice starts stops the run without fuzzing a slice.
"$RUN" --out "$T/h" --workers 2 --slice-seconds 120 --formats png --slices 1 \
  --sync-cmd "rsync -a --delete \"\$OUT/\" \"$T/dest-h/\"" > "$T/h.log" 2>&1 &
pid=$!
sleep 2
start=$(date +%s)
kill -TERM "$pid"; wait "$pid"; rc=$?
check "an early SIGTERM exits 143" test "$rc" -eq 143
check "...well before a 120 s slice could have finished" test $(( $(date +%s) - start )) -lt 100
check "...with no fuzz process left" test -z "$(pgrep -f '/release/decode_png')"

# 10. A directory of the same commit and decoder but another image path is refused.
cp -R "$T/a" "$T/j" && rm -rf "$T/j/.lock" && echo 0000 > "$T/j/image_path_build"
"$RUN" --out "$T/j" "${COMMON[@]}" --slices 1 > "$T/j.log" 2>&1
check "a different image-path identity is refused with exit 3" test $? -eq 3

[ "$fail" -eq 0 ] && echo "all spot-VM checks passed" || { echo "some checks FAILED"; tail -5 "$T"/*.log; }
exit "$fail"
