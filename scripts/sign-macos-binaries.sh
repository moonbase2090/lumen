#!/usr/bin/env bash
# Sign macOS executables or app bundles with the hardened runtime and no
# entitlements, so Bright can be handed to the macOS release gates
# (task-port denial, non-dumpability). It signs and nothing else: no
# notarization, no upload, no release.
#
# usage: sign-macos-binaries.sh [--adhoc] [--identifier-prefix PREFIX] PATH...
#
# Signing in place. The certificate comes from the environment and is never
# printed or written outside a throwaway keychain that is deleted on exit:
#   APPLE_CERTIFICATE_P12       base64 of the Developer ID Application .p12
#   APPLE_CERTIFICATE_PASSWORD  its password
# With either missing the script refuses (exit 1). --adhoc signs with the
# ad-hoc identity instead; it exists for the verifier's tests only, and the
# verifier refuses an ad-hoc signature unless it is also told to allow one.
set -euo pipefail

adhoc=0
identifier_prefix=""
bins=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --adhoc) adhoc=1; shift ;;
    --identifier-prefix)
      [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || {
        echo "sign-macos-binaries: --identifier-prefix needs a value" >&2
        exit 2
      }
      identifier_prefix="$2"
      shift 2
      ;;
    -h|--help) sed -n 2,16p "$0"; exit 0 ;;
    -*) echo "sign-macos-binaries: unknown argument $1" >&2; exit 2 ;;
    *) bins+=("$1"); shift ;;
  esac
done
if [[ ${#bins[@]} -eq 0 ]]; then
  echo "sign-macos-binaries: no binary given" >&2
  exit 2
fi
if [[ "$(uname -s)" != Darwin ]]; then
  echo "sign-macos-binaries: macOS is required" >&2
  exit 1
fi
for b in "${bins[@]}"; do
  [[ -f "$b" || -d "$b" ]] || { echo "sign-macos-binaries: missing path: $b" >&2; exit 1; }
done

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

sign_one() {
  local identity="$1" path="$2"
  shift 2
  local identifier_args=()
  if [[ -n "$identifier_prefix" ]]; then
    identifier_args=(--identifier "$(identifier_for "$path")")
  fi
  codesign --force "$@" --options runtime "${identifier_args[@]}" --sign "$identity" "$path"
}

if [[ "$adhoc" -eq 1 ]]; then
  for b in "${bins[@]}"; do
    sign_one - "$b" --timestamp=none
  done
  echo "sign-macos-binaries: signed ${#bins[@]} binaries ad hoc (tests only)"
  exit 0
fi

if [[ -z "${APPLE_CERTIFICATE_P12:-}" || -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]]; then
  echo "sign-macos-binaries: the signing certificate is not available; refusing to sign" >&2
  exit 1
fi

work="$(mktemp -d)"
keychain="$work/signing.keychain-db"
keychain_password="$(openssl rand -hex 32)"
original_keychains=()
while IFS= read -r line; do
  line="${line//\"/}"
  line="${line#"${line%%[![:space:]]*}"}"
  [[ -n "$line" ]] && original_keychains+=("$line")
done < <(security list-keychains -d user)
cleanup() {
  if ((${#original_keychains[@]})); then
    security list-keychains -d user -s "${original_keychains[@]}" >/dev/null 2>&1 || true
  fi
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

certificate="$work/certificate.p12"
printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode >"$certificate"
security create-keychain -p "$keychain_password" "$keychain" >/dev/null
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
if ! security import "$certificate" -f pkcs12 -k "$keychain" \
  -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null 2>&1; then
  echo "sign-macos-binaries: the signing certificate could not be imported" >&2
  exit 1
fi
security set-key-partition-list -S apple-tool:,apple:,codesign: \
  -s -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" "${original_keychains[@]}"

identities="$(security find-identity -v -p codesigning "$keychain" | grep 'Developer ID Application' || true)"
if [[ "$(printf '%s\n' "$identities" | grep -c .)" -ne 1 ]]; then
  echo "sign-macos-binaries: expected exactly one Developer ID Application identity" >&2
  exit 1
fi
identity="$(printf '%s\n' "$identities" | awk '{print $2}')"

for b in "${bins[@]}"; do
  sign_one "$identity" "$b" --timestamp --keychain "$keychain"
done
echo "sign-macos-binaries: signed ${#bins[@]} binaries with the hardened runtime"
