#!/usr/bin/env bash
# Tests for check-public-text.sh. Each case builds a throwaway git repository
# with the script and one planted file, then checks the exit status. The
# denylist is a fixture, so the test never reads a machine's private list.
set -euo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/check-public-text.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FIXTURE_NAME='Fixture Person'
# The second line matches part of the allowlisted legal name, so a case that
# passes proves the allowlist removed it, not that nothing matched.
printf '%s\n' "$FIXTURE_NAME" 'Moonbase' >"$WORK/denylist.txt"

FAILED=0

# run_case <description> <expected exit> <file content>
run_case() {
  local desc="$1" want="$2" content="$3" repo rc
  repo="$(mktemp -d "$WORK/case.XXXXXX")"
  mkdir -p "$repo/scripts"
  cp "$SCRIPT" "$repo/scripts/check-public-text.sh"
  printf '%s\n' "$content" >"$repo/page.md"
  git -C "$repo" init -q
  git -C "$repo" add -A
  rc=0
  PUBLIC_TEXT_DENYLIST_FILE="$WORK/denylist.txt" PUBLIC_TEXT_DENYLIST= \
    bash "$repo/scripts/check-public-text.sh" >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc (exit $rc)"
  else
    echo "FAIL $desc: want exit $want, got $rc"
    FAILED=1
  fi
}

run_case "planted personal identifier fails" 1 "Contact $FIXTURE_NAME for details."
run_case "identifier beside the allowlisted name on the same line fails" 1 "Moonbase 2090 LLC $FIXTURE_NAME"
run_case "allowlisted name alone passes" 0 "Copyright (c) Moonbase 2090 LLC. All rights reserved."
run_case "a lower-case variant of the legal name is not exempt" 1 "moonbase 2090 llc"

if [[ "$FAILED" -ne 0 ]]; then
  echo "test-check-public-text: FAILED" >&2
  exit 1
fi
echo "test-check-public-text: ok"
