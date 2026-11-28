#!/usr/bin/env bash
# Submit a signed Bright.app or disk image to Apple's notary service, wait for
# the verdict, staple the ticket and check Gatekeeper accepts the result.
#
# usage: notarize-macos.sh APP_OR_DMG
#
# The App Store Connect API key comes from the environment and is never printed;
# it is written to a private temporary file that is removed on exit:
#   APPLE_NOTARY_ISSUER   the issuer id
#   APPLE_NOTARY_KEY_ID   the key id
#   APPLE_NOTARY_KEY      the .p8 private key, as PEM text or base64 of it
# With any of them missing the script refuses (exit 1). It never skips a step:
# a verdict other than Accepted, a failed staple or a Gatekeeper refusal is an
# exit 1, and nothing is published from a path that failed.
#
# Commands under test can be replaced (the tests do): NOTARY_XCRUN and
# NOTARY_SPCTL name a command each.
set -euo pipefail
umask 077

if [[ $# -ne 1 || "$1" == -* ]]; then
  echo "usage: notarize-macos.sh APP_OR_DMG" >&2
  exit 2
fi
target="$1"
XCRUN="${NOTARY_XCRUN:-xcrun}"
SPCTL="${NOTARY_SPCTL:-spctl}"

for name in APPLE_NOTARY_ISSUER APPLE_NOTARY_KEY_ID APPLE_NOTARY_KEY; do
  if [[ -z "${!name:-}" ]]; then
    echo "notarize-macos: $name is not available; refusing to notarize" >&2
    exit 1
  fi
done
kind=app
if [[ "$target" == *.dmg ]]; then
  kind=dmg
  [[ -f "$target" ]] || { echo "notarize-macos: missing disk image: $target" >&2; exit 1; }
else
  [[ -d "$target" && "$(basename "$target")" == Bright.app ]] || {
    echo "notarize-macos: not a Bright.app directory or a .dmg: $target" >&2
    exit 1
  }
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
key="$work/AuthKey.p8"
case "$APPLE_NOTARY_KEY" in
  *'-----BEGIN '*) printf '%s\n' "$APPLE_NOTARY_KEY" >"$key" ;;
  *)
    printf '%s' "$APPLE_NOTARY_KEY" | base64 --decode >"$key" 2>/dev/null || {
      echo "notarize-macos: the notary key is neither PEM text nor base64" >&2
      exit 1
    }
    ;;
esac
grep -Eq -- '-----BEGIN (EC |RSA )?PRIVATE KEY-----' "$key" || {
  echo "notarize-macos: the notary key holds no PEM private key" >&2
  exit 1
}

submit="$target"
if [[ "$kind" == app ]]; then
  submit="$work/Bright-submit.zip"
  if command -v ditto >/dev/null 2>&1; then
    ditto -c -k --keepParent "$target" "$submit"
  else
    (cd "$(dirname "$target")" && zip -qry "$submit" "$(basename "$target")")
  fi
fi

result="$work/result.json"
if ! "$XCRUN" notarytool submit "$submit" --wait \
  --issuer "$APPLE_NOTARY_ISSUER" --key-id "$APPLE_NOTARY_KEY_ID" --key "$key" \
  --output-format json >"$result" 2>"$work/submit.err"; then
  echo "notarize-macos: the notary service refused the submission" >&2
  exit 1
fi
status="$(python3 -c 'import json,sys
try:
    print(json.load(open(sys.argv[1])).get("status", ""))
except (OSError, ValueError):
    print("")' "$result")"
[[ "$status" == Accepted ]] || {
  echo "notarize-macos: notarization was not accepted (status: ${status:-unknown})" >&2
  exit 1
}
echo "notarize-macos: Apple accepted $(basename "$target")"

"$XCRUN" stapler staple "$target" >/dev/null || {
  echo "notarize-macos: stapling the ticket failed" >&2
  exit 1
}
"$XCRUN" stapler validate "$target" >/dev/null || {
  echo "notarize-macos: the stapled ticket does not validate" >&2
  exit 1
}
if [[ "$kind" == dmg ]]; then
  assess=(--assess --type open --context context:primary-signature --verbose=2)
else
  assess=(--assess --type execute --verbose=2)
fi
"$SPCTL" "${assess[@]}" "$target" || {
  echo "notarize-macos: Gatekeeper does not accept $(basename "$target")" >&2
  exit 1
}
echo "notarize-macos: stapled and accepted by Gatekeeper: $(basename "$target")"
