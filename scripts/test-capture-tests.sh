#!/usr/bin/env bash
# Tests for capture-tests.sh --check-only. Each case writes a small log and
# checks the exit status of the line-count check.
set -euo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/capture-tests.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

# run_case <description> <expected exit> <log content>
run_case() {
  local desc="$1" want="$2" content="$3" rc=0
  printf '%s\n' "$content" >"$WORK/log.txt"
  bash "$SCRIPT" "$WORK/log.txt" --check-only >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want)"
    FAILED=1
  fi
}

COMPLETE='running 3 tests
test a ... ok
test b ... ignored, not now
test c ... FAILED
test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 1 test
test d::e ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'

run_case "a complete log passes" 0 "$COMPLETE"
run_case "a log with a per-test line cut out fails" 1 "$(printf '%s\n' "$COMPLETE" | grep -v '^test b ')"
run_case "a log with only the result lines fails" 1 "$(printf '%s\n' "$COMPLETE" | grep -v '^test [a-z:]* \.\.\.')"
run_case "an empty log fails" 1 ""
run_case "a log with per-test lines and no result lines fails" 1 "test a ... ok"

exit "$FAILED"
