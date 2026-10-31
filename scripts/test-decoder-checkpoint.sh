#!/usr/bin/env bash
# Tests the console checkpoints of scripts/decoder-fuzz.sh with real (short) fuzz
# runs: a run writes blocks to a file standing in for the serial console, a
# replacement run restores from the newest one, hours continue, the corpus is
# not carried, and a block of a different build is refused.
# Takes a few minutes. Needs the same toolchain as the campaign.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/scripts/decoder-fuzz.sh"
T="$(mktemp -d)"
trap 'pkill -f -- "$T" 2>/dev/null; rm -rf "$T"' EXIT
export NIGHTLY="${NIGHTLY:-nightly}"
TOOLS=(cargo run -q --locked -p lumen-decoder-tools --)
COMMON=(--workers 2 --slice-seconds 8 --formats png)
fail=0
check() { desc="$1"; shift; if "$@"; then echo "ok    $desc"; else echo "FAIL  $desc"; fail=1; fi; }
hours() { python3 - "$1" <<'PY'
import json,sys
h=0.0
for l in open(sys.argv[1]):
    if l.strip(): h=max(h,json.loads(l)["at_cpu_hour"])
print(h)
PY
}

# 1. A run with a checkpoint after every slice (minutes = 0) and one at the end.
printf 'boot noise\n' > "$T/console.txt"
"$RUN" --out "$T/a" "${COMMON[@]}" --slices 2 --checkpoint-console "$T/console.txt" --checkpoint-minutes 0 \
  --console "$T/console.txt" --console-prefix TESTCK > "$T/a.log" 2>&1
check "the run exits 0" test $? -eq 0
check "three blocks were written (two slices and the end)" test "$(grep -c 'CKPT-BEGIN' "$T/console.txt")" -ge 3
check "the last block is the final one" bash -c "grep CKPT-BEGIN '$T/console.txt' | tail -1 | grep -q 'kind=final'"
check "the status lines are mixed in with the blocks" grep -q '^TESTCK' "$T/console.txt"
h1="$(hours "$T/a/png/samples.jsonl")"

# 2. A replacement run restores from the console text alone.
"$RUN" --out "$T/b" "${COMMON[@]}" --target-cpu-hours 0.01 --restore-console "$T/console.txt" --checkpoint-console "$T/console-b.txt" --checkpoint-minutes 0 > "$T/b.log" 2>&1
check "the restored run exits 0" test $? -eq 0
h2="$(hours "$T/b/png/samples.jsonl")"
check "hours continued past the checkpointed $h1 to at least 0.01 ($h2)" python3 -c "import sys; sys.exit(0 if $h2 >= 0.01 and $h2 > $h1 else 1)"
check "the first restored sample is the checkpointed one" test "$(head -1 "$T/b/png/samples.jsonl")" = "$(head -1 "$T/a/png/samples.jsonl")"
"${TOOLS[@]}" ckpt-decode "$T/console.txt" "$T/last.tgz" > "$T/last.txt"
mkdir "$T/y" && tar xzf "$T/last.tgz" -C "$T/y"
check "the checkpoint holds the crash records and samples as the run left them" bash -c "cmp -s '$T/a/png/crashes.jsonl' '$T/y/png/crashes.jsonl' && cmp -s '$T/a/png/samples.jsonl' '$T/y/png/samples.jsonl'"
check "the sequence number continued past the first run's" test "$(cat "$T/b/ckpt_seq")" -gt "$(cat "$T/a/ckpt_seq")"
check "the block carried no corpus" test "$(tar tzf "$T/last.tgz" | grep -c corpus)" -eq 0
check "...and fits the limit with room to spare" test "$(wc -c < "$T/last.tgz" | tr -d " ")" -lt 17000

# 2b. A console path with quotes and shell syntax in it is data, never a command.
EVIL="$T/it's';touch PWNED-BY-PATH;'x.txt"
rm -f "$ROOT/PWNED-BY-PATH"
cp "$T/console.txt" "$EVIL"
"$RUN" --out "$T/q" "${COMMON[@]}" --target-cpu-hours 0.001 --restore-console "$EVIL" > "$T/q.log" 2>&1
check "a path with an apostrophe and shell syntax restores like any other" test $? -eq 0
check "...and nothing in the path was run" test ! -e "$ROOT/PWNED-BY-PATH" -a ! -e PWNED-BY-PATH
check "...and the restored samples are the checkpointed ones" test "$(head -1 "$T/q/png/samples.jsonl")" = "$(head -1 "$T/a/png/samples.jsonl")"

# 3. A block of a different build is refused, before it touches OUT.
mkdir "$T/x" && tar xzf "$T/last.tgz" -C "$T/x"
echo 0000000000000000000000000000000000000000 > "$T/x/commit"
tar czf "$T/other.tgz" -C "$T/x" . && "${TOOLS[@]}" ckpt-encode TESTCK other 99 final "$T/other.tgz" > "$T/other.txt"
"$RUN" --out "$T/c" "${COMMON[@]}" --slices 1 --restore-console "$T/other.txt" > "$T/c.log" 2>&1
check "a block of a different build exits 3" test $? -eq 3
check "...and leaves OUT without samples" test ! -s "$T/c/png/samples.jsonl"

# 4. Console text without a whole block restores nothing.
printf 'TESTCK CKPT-BEGIN id=q seq=1 kind=final bytes=3 chunks=1 sha256=00\n' > "$T/empty.txt"
"$RUN" --out "$T/d" "${COMMON[@]}" --slices 1 --restore-console "$T/empty.txt" > "$T/d.log" 2>&1
check "no verified block means exit 6 (restore failed)" test $? -eq 6

[ "$fail" = 0 ] && echo "all checkpoint checks passed" || { echo "SOME CHECKS FAILED"; exit 1; }
