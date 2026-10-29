#!/usr/bin/env bash
# Tests for run-comparison.sh. A stand-in for the evidence driver exits with a
# status chosen per mode, so the test checks what the script does with each
# status without running the harness. The output directory has a space in its
# name, to check that paths are quoted.
set -euo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/run-comparison.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

# The stand-in prints its arguments and exits with the status that
# FAKE_STATUS_<mode> names, where the mode is the first argument without its
# dashes (corpus, matrix, flood, cost, parity, decoder, render_log), or
# panes_a and panes_b for the two placements.
cat >"$WORK/fake-evidence" <<'FAKE'
#!/usr/bin/env bash
key="${1#--}"
var="FAKE_STATUS_${key//-/_}"
if [[ "$1" == "--panes" ]]; then
  for ((i = 1; i <= $#; i++)); do
    if [[ "${!i}" == "--placement" ]]; then
      j=$((i + 1))
      var="FAKE_STATUS_panes_${!j}"
    fi
  done
fi
echo "args: $*"
exit "${!var:-0}"
FAKE
chmod +x "$WORK/fake-evidence"

# run_case <description> <expected exit> <expected text in stderr or modes.tsv> [VAR=status ...]
run_case() {
  local desc="$1" want="$2" needle="$3" rc=0 out="$WORK/out dir $RANDOM"
  shift 3
  env "$@" LUMEN_EVIDENCE_BIN="$WORK/fake-evidence" \
    bash "$SCRIPT" --commit abc123 --out "$out" --flood-seconds 5 --skip-tests \
    >"$WORK/stdout" 2>"$WORK/stderr" || rc=$?
  local seen
  seen="$(cat "$WORK/stderr" "$out/modes.tsv" 2>/dev/null || true)"
  if [[ "$rc" -eq "$want" && "$seen" == *"$needle"* ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want, looked for: $needle)"
    FAILED=1
  fi
}

run_case "every mode measured: exit 0" 0 $'parity\t0\tmeasured'
run_case "a mode with nothing to measure is recorded, not failed" 0 $'decoder\t3\tnot measured' FAKE_STATUS_decoder=3
run_case "a broken limit fails the run and names the mode" 1 "FAILED: corpus-b" FAKE_STATUS_corpus=1
run_case "a failing placement is named" 1 "panes-b" FAKE_STATUS_panes_b=1
run_case "later modes still run after a failure" 1 $'panes-a\t0\tmeasured' FAKE_STATUS_corpus=1
run_case "more than one failure names each" 1 "FAILED: corpus-b corpus-a-sealed corpus-a-unsealed" FAKE_STATUS_corpus=2

# --strict is an argument, not an environment variable: check it directly.
rc=0
out="$WORK/out strict"
FAKE_STATUS_parity=3 LUMEN_EVIDENCE_BIN="$WORK/fake-evidence" \
  bash "$SCRIPT" --commit abc123 --out "$out" --flood-seconds 5 --skip-tests --strict \
  >/dev/null 2>"$WORK/stderr" || rc=$?
if [[ "$rc" -eq 1 && "$(cat "$WORK/stderr")" == *"FAILED: parity"* ]]; then
  echo "ok   --strict fails a not-measured mode"
else
  echo "FAIL --strict fails a not-measured mode (exit $rc)"
  FAILED=1
fi

# Required arguments.
rc=0
bash "$SCRIPT" --commit abc123 >/dev/null 2>&1 || rc=$?
if [[ "$rc" -eq 2 ]]; then echo "ok   missing arguments are a usage error"; else echo "FAIL usage (exit $rc)"; FAILED=1; fi
rc=0
bash "$SCRIPT" --commit abc --out "$WORK/o" --flood-seconds ten >/dev/null 2>&1 || rc=$?
if [[ "$rc" -eq 2 ]]; then echo "ok   a non-numeric flood length is a usage error"; else echo "FAIL flood length (exit $rc)"; FAILED=1; fi

# Paths with a space reach the driver whole, and both placements run.
out="$WORK/out space check"
LUMEN_EVIDENCE_BIN="$WORK/fake-evidence" bash "$SCRIPT" --commit abc123 --out "$out" --flood-seconds 7 --skip-tests >/dev/null 2>&1
if grep -q -- "--out $out/panes-a" "$out/panes-a.out" && grep -q -- "--placement b" "$out/panes-b.out" \
  && grep -q -- "--seconds 7" "$out/flood-b.out"; then
  echo "ok   paths with a space, both placements and the flood length reach the driver"
else
  echo "FAIL arguments reaching the driver"
  FAILED=1
fi

exit "$FAILED"
