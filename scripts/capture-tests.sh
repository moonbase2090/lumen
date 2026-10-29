#!/usr/bin/env bash
# Run the workspace tests and keep the full output in a file, then check that
# the file holds one line for every test the run counted.
#
# usage: scripts/capture-tests.sh OUT_FILE [cargo test arguments]
#
# The output goes straight from cargo into OUT_FILE. It is never piped through
# a console or a pager, which is where per-test lines were lost before. The
# check then compares the per-test lines (`test NAME ... ok|FAILED|ignored`)
# with the totals on the `test result:` lines. A log that was cut short has
# fewer lines than the totals and fails the check.
#
# Exit status: cargo's own status if the tests failed, 3 if the log does not
# hold every test line, 2 for a usage error, otherwise 0.
set -uo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 OUT_FILE [cargo test arguments]" >&2
  exit 2
fi
out="$1"
shift
args=("$@")
if [[ ${#args[@]} -eq 0 ]]; then
  args=(--locked --workspace)
fi

# check_log FILE: print the counts and return 0 when every counted test has a line.
check_log() {
  awk '
    /^test .* \.\.\. (ok|FAILED|ignored)/ { lines++ }
    /^test result: / {
      for (i = 1; i <= NF; i++) {
        if ($(i + 1) ~ /^passed;?$/ || $(i + 1) ~ /^failed;?$/ || $(i + 1) ~ /^ignored;?$/) total += $i
      }
    }
    END {
      printf "capture check: %d per-test lines, %d tests counted by the result lines\n", lines, total
      exit (lines == total && total > 0) ? 0 : 1
    }
  ' "$1"
}

if [[ "${1:-}" == "--check-only" ]]; then
  check_log "$out"
  exit $?
fi

{
  rustc -V
  uname -sr
  printf '== cargo test %s\n' "${args[*]}"
} >"$out"

rc=0
cargo test "${args[@]}" >>"$out" 2>&1 || rc=$?

if [[ "$rc" -ne 0 ]]; then
  echo "cargo test exited $rc; see $out" >&2
  check_log "$out" >&2 || true
  exit "$rc"
fi
if ! check_log "$out"; then
  echo "the log is missing per-test lines; do not use it as evidence" >&2
  exit 3
fi
