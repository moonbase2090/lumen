#!/usr/bin/env bash
# Tests for verify-release-set.sh, on stand-in files. The one passing case shows
# the verifier is not refusing everything.
set -euo pipefail

VERIFY="$(cd "$(dirname "$0")" && pwd)/verify-release-set.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

# set_up NAME: a good release set in $WORK/NAME.
set_up() {
  local d="$WORK/$1"
  mkdir -p "$d"
  printf 'terminal with a window\n' >"$d/lumen-term"
  printf 'worker\n' >"$d/lumen-decoder-worker"
  printf 'holder\n' >"$d/lumen-holder"
  chmod +x "$d/lumen-term" "$d/lumen-decoder-worker" "$d/lumen-holder"
}

# expect DESCRIPTION WANT DIR
expect() {
  local desc="$1" want="$2" rc=0
  shift 2
  "$VERIFY" "$@" >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want)"
    FAILED=1
  fi
}

set_up good
expect "a complete set passes" 0 "$WORK/good"

set_up noworker; rm "$WORK/noworker/lumen-decoder-worker"
expect "a set without the decoder worker is refused" 1 "$WORK/noworker"

set_up noterm; rm "$WORK/noterm/lumen-term"
expect "a set without the terminal is refused" 1 "$WORK/noterm"

set_up noholder; rm "$WORK/noholder/lumen-holder"
expect "a set without the holder is refused" 1 "$WORK/noholder"

set_up nowindow
printf 'this build has no window: build with --features window\n' >>"$WORK/nowindow/lumen-term"
expect "a terminal built without the window feature is refused" 1 "$WORK/nowindow"

set_up notexec; chmod -x "$WORK/notexec/lumen-decoder-worker"
expect "a worker that is not executable is refused" 1 "$WORK/notexec"

set_up hostile; cp "$WORK/hostile/lumen-decoder-worker" "$WORK/hostile/lumen-decoder-hostile"
expect "the test-only hostile worker in the set is refused" 1 "$WORK/hostile"

set_up link; rm "$WORK/link/lumen-decoder-worker"; ln -s "$WORK/good/lumen-decoder-worker" "$WORK/link/lumen-decoder-worker"
expect "a worker that is a link is refused" 1 "$WORK/link"

expect "a missing directory is refused" 1 "$WORK/nope"
expect "no argument is a usage error" 2
expect "an option is a usage error" 2 --bogus

exit "$FAILED"
