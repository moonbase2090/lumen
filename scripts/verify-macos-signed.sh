#!/usr/bin/env bash
# Refuse a macOS binary that is not a hardened-runtime release signature.
# A binary passes only if every check below holds; the first failure exits 1.
#   - it is signed and the signature verifies (strict)
#   - the hardened runtime flag is set
#   - it is not ad hoc, and it has a team identifier (a Developer ID signature)
#   - it carries no get-task-allow entitlement, which would let another
#     process of the same user take its task port
#
# usage: verify-macos-signed.sh [--allow-adhoc] BINARY...
# --allow-adhoc skips the ad-hoc and team identifier checks. It is for tests.
set -euo pipefail

allow_adhoc=0
bins=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-adhoc) allow_adhoc=1; shift ;;
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

for b in "${bins[@]}"; do
  name="$(basename "$b")"
  [[ -f "$b" ]] || fail "$name" "missing"
  codesign --verify --strict "$b" >/dev/null 2>&1 || fail "$name" "not signed, or the signature does not verify"
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
  # The entitlements, if any, are read from the signature itself.
  if codesign -d --entitlements - --xml "$b" 2>/dev/null | grep -aq 'get-task-allow'; then
    fail "$name" "carries the get-task-allow entitlement"
  fi
  echo "verify-macos-signed: $name ok"
done
