#!/usr/bin/env bash
# Refuse a macOS executable or app bundle that is not a hardened-runtime
# release signature. A path passes only if every check below holds.
#   - it is signed and the signature verifies (strict)
#   - the hardened runtime flag is set
#   - it is not ad hoc, and it has a team identifier (a Developer ID signature)
#   - it carries no get-task-allow entitlement, which would let another
#     process of the same user take its task port
#
# usage: verify-macos-signed.sh [--allow-adhoc] [--identifier-prefix PREFIX] PATH...
# --allow-adhoc skips the ad-hoc and team identifier checks. It is for tests.
set -euo pipefail

allow_adhoc=0
identifier_prefix=""
bins=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-adhoc) allow_adhoc=1; shift ;;
    --identifier-prefix)
      [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || {
        echo "verify-macos-signed: --identifier-prefix needs a value" >&2
        exit 2
      }
      identifier_prefix="$2"
      shift 2
      ;;
    -h|--help) sed -n 2,13p "$0"; exit 0 ;;
    -*) echo "verify-macos-signed: unknown argument $1" >&2; exit 2 ;;
    *) bins+=("$1"); shift ;;
  esac
done
if [[ ${#bins[@]} -eq 0 ]]; then
  echo "verify-macos-signed: no binary given" >&2
  exit 2
fi

fail() { echo "verify-macos-signed: $1: $2" >&2; exit 1; }

identifier_for() {
  local name
  name="$(basename "$1")"
  case "$name" in
    Bright.app|bright) printf '%s\n' "$identifier_prefix" ;;
    lumen-decoder-worker) printf '%s.decoder\n' "$identifier_prefix" ;;
    lumen-holder) printf '%s.holder\n' "$identifier_prefix" ;;
    *) printf '%s.%s\n' "$identifier_prefix" "$name" ;;
  esac
}

for b in "${bins[@]}"; do
  name="$(basename "$b")"
  [[ -f "$b" || -d "$b" ]] || fail "$name" "missing"
  verify_args=(--verify --strict)
  [[ -d "$b" ]] && verify_args+=(--deep)
  codesign "${verify_args[@]}" "$b" >/dev/null 2>&1 || fail "$name" "not signed, or the signature does not verify"
  info="$(codesign -dvv "$b" 2>&1)"
  flags="$(printf '%s\n' "$info" | sed -n 's/^CodeDirectory .*flags=0x[0-9a-f]*(\(.*\)) hashes.*/\1/p')"
  case ",$flags," in
    *,runtime,*) ;;
    *) fail "$name" "the hardened runtime flag is not set" ;;
  esac
  if [[ "$allow_adhoc" -eq 0 ]]; then
    printf '%s\n' "$info" | grep -q '^Signature=adhoc' && fail "$name" "ad-hoc signature"
    team="$(printf '%s\n' "$info" | sed -n 's/^TeamIdentifier=//p')"
    [[ -n "$team" && "$team" != "not set" ]] || fail "$name" "no team identifier"
    printf '%s\n' "$info" | grep -q '^Authority=Developer ID Application' || fail "$name" "not a Developer ID Application signature"
  fi
  if [[ -n "$identifier_prefix" ]]; then
    identifier="$(printf '%s\n' "$info" | sed -n 's/^Identifier=//p')"
    expected="$(identifier_for "$b")"
    [[ "$identifier" == "$expected" ]] || fail "$name" "identifier is $identifier, expected $expected"
  fi
  # The entitlements, if any, are read from the signature itself.
  if codesign -d --entitlements - --xml "$b" 2>/dev/null | grep -aq 'get-task-allow'; then
    fail "$name" "carries the get-task-allow entitlement"
  fi
  echo "verify-macos-signed: $name ok"
done
