#!/usr/bin/env bash
# Tests for scripts/gen-default-ignorable.py that need no network: it must refuse
# a file that is not the pinned Unicode data, and print usage with no argument.
set -uo pipefail
SCRIPT="$(cd "$(dirname "$0")" && pwd)/gen-default-ignorable.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

echo "not unicode data" >"$WORK/bad.txt"
rc=0
python3 "$SCRIPT" --check "$WORK/bad.txt" >/dev/null 2>"$WORK/err" || rc=$?
if [[ "$rc" -eq 2 ]] && grep -q "not the pinned file" "$WORK/err"; then
  echo "ok   a file that is not the pinned data is refused"
else
  echo "FAIL a wrong file (exit $rc)"; FAILED=1
fi

# The right header with the wrong content still fails on the digest.
printf '# DerivedCoreProperties-16.0.0.txt\n00AD ; Default_Ignorable_Code_Point # x\n' >"$WORK/forged.txt"
rc=0
python3 "$SCRIPT" "$WORK/forged.txt" >/dev/null 2>"$WORK/err" || rc=$?
if [[ "$rc" -eq 2 ]] && grep -q "sha256" "$WORK/err"; then
  echo "ok   the right header with other content is refused by its digest"
else
  echo "FAIL a forged file (exit $rc)"; FAILED=1
fi

rc=0
python3 "$SCRIPT" >/dev/null 2>&1 || rc=$?
if [[ "$rc" -eq 2 ]]; then echo "ok   no argument is a usage error"; else echo "FAIL usage (exit $rc)"; FAILED=1; fi

exit "$FAILED"
