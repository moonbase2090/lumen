#!/usr/bin/env bash
# Run the whole phase 0.5 comparison harness at one commit and keep every log.
#
# usage: scripts/run-comparison.sh --commit SHA --out DIR --flood-seconds N
#          [--cost-base REF] [--ledger FILE] [--render-log FILE] [--skip-tests]
#          [--strict]
#
# It builds the evidence driver once, runs the tests through capture-tests.sh,
# then runs every mode in turn: the red-team corpora (rt-1 to rt-4) through
# each arm, reach, flood, cost, parity, the decoder ledger, the render log and
# the pane corpus under both placements. It waits for each mode, never puts one
# in the background, and quotes every path, so a directory with a space in it
# works.
#
# Each mode writes run.jsonl and summary.txt under DIR/<mode>/, and its console
# output goes to DIR/<mode>.out. DIR/modes.tsv has one line per mode: its name,
# its exit status and what that means.
#
# A mode exits 0 when it measured and held, 3 when it had nothing to measure
# (no ledger, no render log, no declared channel), and 1 or more when a limit
# was broken or the run failed. This script treats 3 as "not measured": it is
# recorded and does not fail the run, unless --strict is given. Any other
# non-zero status fails the run, and the script exits 1 after the last mode
# with the failed modes named on stderr.
#
# The evidence driver binary can be replaced with LUMEN_EVIDENCE_BIN, which the
# script's own test uses.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COST_BASE="5549359"
commit="" out="" flood="" ledger="" renderlog="" skip_tests=0 strict=0

usage() {
  sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --commit) commit="${2:-}"; shift 2 ;;
    --out) out="${2:-}"; shift 2 ;;
    --flood-seconds) flood="${2:-}"; shift 2 ;;
    --cost-base) COST_BASE="${2:-}"; shift 2 ;;
    --ledger) ledger="${2:-}"; shift 2 ;;
    --render-log) renderlog="${2:-}"; shift 2 ;;
    --skip-tests) skip_tests=1; shift ;;
    --strict) strict=1; shift ;;
    -h | --help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

if [[ -z "$commit" || -z "$out" || -z "$flood" ]]; then
  echo "--commit, --out and --flood-seconds are required" >&2
  usage
fi
if ! [[ "$flood" =~ ^[0-9]+$ ]]; then
  echo "--flood-seconds must be a whole number" >&2
  exit 2
fi

mkdir -p "$out" || exit 2
: >"$out/modes.tsv"
failed=()

# record <name> <status> <meaning>
record() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$out/modes.tsv"
}

# run_mode <name> <command...>: run the command, wait for it, record its status.
run_mode() {
  local name="$1" status=0
  shift
  echo "== $name"
  "$@" >"$out/$name.out" 2>&1 || status=$?
  case "$status" in
    0) record "$name" 0 "measured" ;;
    3)
      if [[ "$strict" -eq 1 ]]; then
        record "$name" 3 "not measured (fails under --strict)"
        failed+=("$name")
      else
        record "$name" 3 "not measured"
      fi
      ;;
    *)
      record "$name" "$status" "failed"
      failed+=("$name")
      ;;
  esac
}

cd "$ROOT" || exit 2

if [[ -n "${LUMEN_EVIDENCE_BIN:-}" ]]; then
  BIN="$LUMEN_EVIDENCE_BIN"
else
  echo "== build"
  if ! cargo build --release --locked -p lumen-evidence >"$out/build.out" 2>&1; then
    record build 1 "failed"
    echo "build failed; see $out/build.out" >&2
    exit 1
  fi
  record build 0 "built"
  BIN="$ROOT/target/release/lumen-evidence"
fi

if [[ "$skip_tests" -eq 0 ]]; then
  # capture-tests.sh exits 3 when its log lost per-test lines, which is a
  # failure here, not "not measured".
  echo "== tests"
  status=0
  "$ROOT/scripts/capture-tests.sh" "$out/tests.txt" >"$out/tests.out" 2>&1 || status=$?
  if [[ "$status" -eq 0 ]]; then
    record tests 0 "measured"
  else
    record tests "$status" "failed"
    failed+=("tests")
  fi
fi

corpora=(
  --corpus "$ROOT/docs/phase-0.5/corpus"
  --corpus "$ROOT/docs/phase-0.5/corpus-rt-2"
  --corpus "$ROOT/docs/phase-0.5/corpus-rt-3"
  --corpus "$ROOT/docs/phase-0.5/corpus-rt-4"
)

for arm in b a-sealed a-unsealed; do
  run_mode "corpus-$arm" "$BIN" "${corpora[@]}" --commit "$commit" --out "$out/corpus-$arm" --arm "$arm"
done
for arm in b a; do
  run_mode "reach-$arm" "$BIN" --matrix "$ROOT/docs/phase-0.5/session-matrix.json" --commit "$commit" --out "$out/reach-$arm" --arm "$arm"
done
for arm in b a-sealed a-unsealed; do
  run_mode "flood-$arm" "$BIN" --flood --seconds "$flood" --commit "$commit" --out "$out/flood-$arm" --arm "$arm"
done

run_mode cost "$BIN" --cost --root "$ROOT" --base "$COST_BASE" --commit "$commit" --out "$out/cost"
run_mode parity "$BIN" --parity --channels "$ROOT/docs/phase-0.5/channels.json" --commit "$commit" --out "$out/parity"

if [[ -n "$ledger" ]]; then
  run_mode decoder "$BIN" --decoder --ledger "$ledger" --commit "$commit" --out "$out/decoder"
else
  run_mode decoder "$BIN" --decoder --commit "$commit" --out "$out/decoder"
fi
if [[ -n "$renderlog" ]]; then
  run_mode render-log "$BIN" --render-log --log "$renderlog" --commit "$commit" --out "$out/render-log"
else
  run_mode render-log "$BIN" --render-log --commit "$commit" --out "$out/render-log"
fi

for placement in a b; do
  run_mode "panes-$placement" "$BIN" --panes "$ROOT/docs/phase-0.5/corpus-panes" --placement "$placement" --commit "$commit" --out "$out/panes-$placement"
done

echo "== modes"
cat "$out/modes.tsv"
if [[ ${#failed[@]} -gt 0 ]]; then
  echo "FAILED: ${failed[*]}" >&2
  exit 1
fi
