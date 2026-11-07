#!/usr/bin/env bash
# Tests for release-check.sh. Each case builds a throwaway tree with the script,
# a ledger, and stubs for the scanner, the identity tool and the judge, then
# checks the exit status. The one passing case proves the stubs are not what
# makes the others fail.
set -euo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/release-check.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

GOOD='{"revision":"fz-1","decoder_build":"d1","image_path_build":"p1",
 "formats":[{"name":"png","cpu_hours":1200,"crashes":[],"coverage":[]},
            {"name":"webp","cpu_hours":1200,"crashes":[],"coverage":[]}],
 "sandbox_escape":{"revision":"se-1","passed":3,"total":3}}'
CRASHED='{"decoder_build":"d1","image_path_build":"p1",
 "formats":[{"name":"png","cpu_hours":900,"crashes":[{"at_cpu_hour":300,"kind":"memory"}],"coverage":[]}]}'

FAILED=0

# run_case <description> <expected exit> [VAR=value ...]
# Variables: LEDGER_TEXT (file content; unset = no file), FLAGS (unset = no file),
# DEC_ID, IMG_ID (identities the stub reports), PII_RC, JUDGE_RC, ID_RC, ARGS.
run_case() {
  local desc="$1" want="$2" repo rc
  shift 2
  repo="$(mktemp -d "$WORK/case.XXXXXX")"
  mkdir -p "$repo/scripts" "$repo/release"
  cp "$SCRIPT" "$repo/scripts/release-check.sh"
  git -C "$repo" init -q
  local LEDGER_TEXT="$GOOD" FLAGS="fuzz flags" DEC_ID=d1 IMG_ID=p1 PII_RC=0 JUDGE_RC=0 ID_RC=0 ARGS=""
  local unset_ledger=0 unset_flags=0
  for kv in "$@"; do
    case "$kv" in
      NOLEDGER) unset_ledger=1 ;;
      NOFLAGS) unset_flags=1 ;;
      LEDGER_TEXT=*) LEDGER_TEXT="${kv#LEDGER_TEXT=}" ;;
      FLAGS=*) FLAGS="${kv#FLAGS=}" ;;
      DEC_ID=*) DEC_ID="${kv#DEC_ID=}" ;;
      IMG_ID=*) IMG_ID="${kv#IMG_ID=}" ;;
      PII_RC=*) PII_RC="${kv#PII_RC=}" ;;
      JUDGE_RC=*) JUDGE_RC="${kv#JUDGE_RC=}" ;;
      ID_RC=*) ID_RC="${kv#ID_RC=}" ;;
      ARGS=*) ARGS="${kv#ARGS=}" ;;
    esac
  done
  [[ "$unset_ledger" -eq 1 ]] || printf '%s\n' "$LEDGER_TEXT" >"$repo/release/decoder-ledger.json"
  [[ "$unset_flags" -eq 1 ]] || printf '%s' "$FLAGS" >"$repo/release/decoder-fuzz-flags.txt"
  # Stubs: the scanner and judge exit as told; the identity tool reports the
  # id for the kind it is asked for and records the flags it was given.
  printf '#!/bin/sh\nexit %s\n' "$PII_RC" >"$repo/pii-stub"
  printf '#!/bin/sh\nexit %s\n' "$JUDGE_RC" >"$repo/judge-stub"
  cat >"$repo/id-stub" <<STUB
#!/bin/sh
[ "$ID_RC" -eq 0 ] || exit $ID_RC
[ "\$2" = "$FLAGS" ] || { echo wrong-flags; exit 0; }
case "\$3" in decoder) echo "$DEC_ID" ;; image-path) echo "$IMG_ID" ;; esac
STUB
  chmod +x "$repo/pii-stub" "$repo/judge-stub" "$repo/id-stub"
  rc=0
  # shellcheck disable=SC2086
  RELEASE_CHECK_PII="$repo/pii-stub" RELEASE_CHECK_JUDGE="$repo/judge-stub" \
    RELEASE_CHECK_IDENTITY="$repo/id-stub" \
    bash "$repo/scripts/release-check.sh" $ARGS >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc (exit $rc)"
  else
    echo "FAIL $desc: want exit $want, got $rc"
    FAILED=1
  fi
}

run_case "everything holds: passes" 0
run_case "scanner failure refuses" 1 PII_RC=1
run_case "no ledger refuses" 1 NOLEDGER
run_case "a ledger that is not JSON refuses" 1 "LEDGER_TEXT=not json"
run_case "a ledger with no format refuses" 1 'LEDGER_TEXT={"decoder_build":"d1","image_path_build":"p1","formats":[]}'
run_case "a ledger without the decoder identity refuses" 1 'LEDGER_TEXT={"image_path_build":"p1","formats":[{"name":"png","crashes":[]}]}'
run_case "a ledger whose format has no crash list refuses" 1 'LEDGER_TEXT={"decoder_build":"d1","image_path_build":"p1","formats":[{"name":"png"}]}'
run_case "an open crash refuses even when the judge is stubbed to pass" 1 "LEDGER_TEXT=$CRASHED"
run_case "a decoder identity mismatch refuses" 1 DEC_ID=d2
run_case "an image-path identity mismatch refuses" 1 IMG_ID=p2
run_case "no fuzz flags file refuses" 1 NOFLAGS
run_case "an empty fuzz flags file refuses" 1 FLAGS=
run_case "flags that differ from the recorded ones change the identity and refuse" 1 "FLAGS=other flags" DEC_ID=wrong-flags
run_case "an identity tool that fails refuses" 1 ID_RC=4
run_case "a judge that fails the gate refuses" 1 JUDGE_RC=1
run_case "a judge that measured nothing refuses" 1 JUDGE_RC=3
run_case "a judge usage error refuses" 1 JUDGE_RC=2
run_case "an unknown argument is a usage error" 2 ARGS=--waive
run_case "explicit ledger and flags paths are accepted" 0 "ARGS=--ledger release/decoder-ledger.json --fuzz-flags-file release/decoder-fuzz-flags.txt"
run_case "a named ledger that does not exist refuses" 1 ARGS=--ledger\ release/missing.json

if [[ "$FAILED" -ne 0 ]]; then
  echo "test-release-check: FAILED" >&2
  exit 1
fi
echo "test-release-check: ok"
