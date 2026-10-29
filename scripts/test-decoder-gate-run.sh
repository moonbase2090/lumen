#!/usr/bin/env bash
# Tests the gate-run controller of scripts/decoder-fuzz.sh with real short fuzz
# runs. The gate's own numbers (1000 CPU-hours, 168) are far out of reach of a
# test, so the driver is given test-only thresholds through the environment
# (LUMEN_GATE_MIN_HOURS, LUMEN_GATE_WINDOW_HOURS); the unit tests in
# lumen-decoder-tools check the real numbers against the judge.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/scripts/decoder-fuzz.sh"
T="$(mktemp -d)"
# The whole test is time-bounded: a driver that spins (the bug this guards against)
# must fail the test, not hang it.
( sleep "${TEST_TIMEOUT:-1500}"; echo "TEST TIMEOUT after ${TEST_TIMEOUT:-1500} s" >&2; kill -TERM $$ ) &
WATCHDOG=$!
ART="$ROOT/crates/lumen-decoder/fuzz/artifacts"
if [ -d "$ART" ]; then mv "$ART" "$T/art-backup"; fi
restore_art() { rm -rf "$ART"; if [ -d "$T/art-backup" ]; then mv "$T/art-backup" "$ART"; fi; }
trap 'kill "$WATCHDOG" 2>/dev/null; pkill -f "/release/decode_" 2>/dev/null; restore_art; rm -rf "$T"' EXIT
export NIGHTLY="${NIGHTLY:-nightly}"
fail=0
check() { desc="$1"; shift; if "$@"; then echo "ok    $desc"; else echo "FAIL  $desc"; fail=1; fi; }
jq_() { python3 -c "import json,sys; d=json.load(open('$1')); print(eval(sys.argv[1]))" "$2"; }
FMT=png,webp
SMALL=(--workers 2 --slice-seconds 6 --formats $FMT --gate-run)

# 1. Stops on the gate holding: tiny thresholds, a generous cap.
LUMEN_GATE_MIN_HOURS=0.004 LUMEN_GATE_WINDOW_HOURS=0.002 "$RUN" --out "$T/a" "${SMALL[@]}" --cap-cpu-hours 0.2 \
  --stats-dir "$T/a-stats" --console "$T/a-console" --finish-cmd "date > \"$T/finished-a\"" > "$T/a.log" 2>&1
check "a gate run ends with exit 0" test $? -eq 0
check "the finish command ran" test -f "$T/finished-a"
check "status.json has both formats" test "$(jq_ "$T/a/status.json" "sorted(d['formats'])")" = "['png', 'webp']"
check "every metric a dashboard needs is there" python3 -c "
import json
d=json.load(open('$T/a/status.json'))
need={'ExecsPerSec','Coverage','CorpusSize','Crashes','HoursSinceNewCrash','HoursSinceCoverageGain','CpuHours','GateHolds'}
raise SystemExit(0 if all(need <= set(f) for f in d['formats'].values()) else 1)"
check "each format stopped by itself (done or capped), none still running" python3 -c "
import json
d=json.load(open('$T/a/status.json'))
raise SystemExit(0 if all(f['state'] in ('done','capped') for f in d['formats'].values()) else 1)"
check "slices alternated between formats (round-robin), in the order they ran" python3 -c "
rows=[l.split()[0] for l in open('$T/a/slices.log')]
raise SystemExit(0 if len(rows)>=4 and rows[:4]==['png','webp','png','webp'] else 1)"
check "a format that held stopped before the cap" python3 -c "
import json
d=json.load(open('$T/a/status.json'))
raise SystemExit(0 if any(f['state']=='done' and f['CpuHours']<0.2 for f in d['formats'].values()) else 1)"

# 2. Measured CPU time is what is counted, and it is not the nominal figure.
check "the counted hours are measured CPU time, different from workers x wall" python3 -c "
import json
d=json.load(open('$T/a/status.json'))['formats']['png']
raise SystemExit(0 if d['CpuHours']>0 and abs(d['CpuHours']-d['NominalWorkerHours'])>1e-6 else 1)"

# 2b. The metrics files and the console lines a launcher reads.
check "<fmt>.env exists for both formats with the six keys" python3 -c "
import re
for f in ('png','webp'):
    t=open('$T/a-stats/%s.env'%f).read()
    keys=dict(l.split('=',1) for l in t.split())
    need={'EXECS_PER_SEC','COVERAGE','CORPUS_SIZE','CRASHES','LAST_CRASH_EPOCH','LAST_COV_EPOCH'}
    assert need <= set(keys), keys
    assert all(re.fullmatch(r'[0-9.]+', keys[k]) for k in need), keys
"
check "the .env files can be sourced by bash, as the metrics script does" bash -c "source '$T/a-stats/png.env' && [ -n \"\$COVERAGE\" ] && [ \"\$CRASHES\" -ge 0 ]"
check "the epochs are plausible (not zero, not in the future)" python3 -c "
import time
t=dict(l.split('=',1) for l in open('$T/a-stats/png.env').read().split())
now=time.time()
assert 0 < int(t['LAST_CRASH_EPOCH']) <= now and 0 < int(t['LAST_COV_EPOCH']) <= now, t"
check "with no crash the last-crash epoch is the campaign start, so its age is the time run (not zero)" python3 -c "
t=dict(l.split('=',1) for l in open('$T/a-stats/png.env').read().split())
c=int(t['CRASHES'])
import json
first=json.loads(open('$T/a/png/samples.jsonl').readline())['at_epoch']
assert c>0 or int(t['LAST_CRASH_EPOCH'])==first, (c,t,first)"
check "the console file has one LTM51-STATUS line per format and slice, and an overall one" python3 -c "
L=[l for l in open('$T/a-console') if l.startswith('LTM51-STATUS')]
assert any(' fmt=png ' in l for l in L) and any(' fmt=webp ' in l for l in L), L[:3]
assert any(' overall ' in l for l in L)
assert all(' EPOCH=' in l for l in L)
assert max(len(l) for l in L) < 400, 'a line must stay short for the 64 KiB console'"
check "samples and crashes carry wall-clock epochs" python3 -c "
import json
r=[json.loads(l) for l in open('$T/a/png/samples.jsonl')]
assert r and all('at_epoch' in x for x in r)"

# 3. The cap: thresholds that cannot be met, a small cap.
LUMEN_GATE_MIN_HOURS=100 LUMEN_GATE_WINDOW_HOURS=50 "$RUN" --out "$T/b" "${SMALL[@]}" --cap-cpu-hours 0.012 > "$T/b.log" 2>&1
check "a run that cannot meet the gate ends at the cap with exit 0" test $? -eq 0
check "both formats are capped, none went far past the cap" python3 -c "
import json
d=json.load(open('$T/b/status.json'))['formats']
raise SystemExit(0 if all(f['state']=='capped' and 0.012<=f['CpuHours']<0.012+0.01 for f in d.values()) else 1)"

# 4. Pause on a crash after the pause hour: exit 7, the finish command does not run.
mkdir -p "$ART/decode_png"
LUMEN_GATE_MIN_HOURS=100 LUMEN_GATE_WINDOW_HOURS=50 "$RUN" --out "$T/c" "${SMALL[@]}" --cap-cpu-hours 5 --pause-on-crash-after 0 \
  --finish-cmd "date > \"$T/finished-c\"" > "$T/c.log" 2>&1 &
pid=$!
for _ in $(seq 1 180); do grep -qE '^#[0-9]+: cov:' "$T/c/png/last.log" 2>/dev/null && break; sleep 1; done
echo "not a real crash" > "$ART/decode_png/crash-gate-test"
wait "$pid"; rc=$?
check "a crash pauses the run with exit 7" test "$rc" -eq 7
check "the finish command did NOT run on a pause" test ! -e "$T/finished-c"
check "the ledger was still written" test -s "$T/c/ledger.json"
check "status says paused and why" python3 -c "
import json
d=json.load(open('$T/c/status.json'))
raise SystemExit(0 if d['state'].startswith('paused') and 'crash' in d['state'] else 1)"
rm -f "$ART/decode_png/crash-gate-test"

# 5. A crash BEFORE the pause hour does not pause (here the pause hour is large).
mkdir -p "$ART/decode_png"
LUMEN_GATE_MIN_HOURS=100 LUMEN_GATE_WINDOW_HOURS=50 "$RUN" --out "$T/d" "${SMALL[@]}" --cap-cpu-hours 0.012 --pause-on-crash-after 1032 > "$T/d.log" 2>&1 &
pid=$!
for _ in $(seq 1 180); do grep -qE '^#[0-9]+: cov:' "$T/d/png/last.log" 2>/dev/null && break; sleep 1; done
echo "not a real crash" > "$ART/decode_png/crash-gate-test2"
wait "$pid"; rc=$?
check "an early crash is recorded but does not pause (exit 0)" test "$rc" -eq 0
check "...and is in the crashes list" test "$(wc -l < "$T/d/png/crashes.jsonl" | tr -d ' ')" -ge 1
rm -f "$ART/decode_png/crash-gate-test2"

# 6. The wall-clock limit.
LUMEN_GATE_MIN_HOURS=100 LUMEN_GATE_WINDOW_HOURS=50 "$RUN" --out "$T/e" "${SMALL[@]}" --cap-cpu-hours 5 --max-wall-hours 0.0003 \
  --finish-cmd "date > \"$T/finished-e\"" > "$T/e.log" 2>&1
check "the wall-clock limit stops the run with exit 8" test $? -eq 8
check "...and does not run the finish command" test ! -e "$T/finished-e"

# 7. A fuzzer that cannot start must stop the run (exit 9), not spin. Each run is
# watched: if it is still going after 150 s it spun, and the check fails.
spin_check() { # name, args...
  name="$1"; shift
  NIGHTLY=no-such-toolchain "$RUN" --out "$T/np-$name" --workers 2 --slice-seconds 5 --formats png "$@" > "$T/np-$name.log" 2>&1 &
  p=$!
  for _ in $(seq 1 150); do kill -0 "$p" 2>/dev/null || break; sleep 1; done
  if kill -0 "$p" 2>/dev/null; then kill -TERM "$p" 2>/dev/null; sleep 2; kill -KILL "$p" 2>/dev/null; wait "$p" 2>/dev/null; echo 124; else wait "$p"; echo $?; fi
}
rc="$(spin_check target --target-cpu-hours 0.01)"
check "--target-cpu-hours with no working fuzzer stops with exit 9 (it used to spin forever)" test "$rc" -eq 9
rc="$(spin_check gate --gate-run --cap-cpu-hours 0.01)"
check "--gate-run with no working fuzzer stops with exit 9" test "$rc" -eq 9
check "the stop is explained" grep -q "counted nothing" "$T/np-gate.log"

[ "$fail" -eq 0 ] && echo "all gate-run checks passed" || { echo "some checks FAILED"; tail -5 "$T"/*.log; }
exit "$fail"
