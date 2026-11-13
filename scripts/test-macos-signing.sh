#!/usr/bin/env bash
# Tests for sign-macos-binaries.sh and verify-macos-signed.sh. They run on macOS
# with a tiny compiled program and ad-hoc signatures, so no certificate is
# needed. A Developer ID signature cannot be made here, so the verifier's
# release-mode acceptance of one is not covered: what is covered is that the
# verifier refuses everything that is not one (unsigned, no hardened runtime,
# get-task-allow, ad hoc) and accepts the hardened-runtime shape under
# --allow-adhoc, which proves it is not refusing everything.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "test-macos-signing: skipped, macOS is required"
  exit 0
fi
DIR="$(cd "$(dirname "$0")" && pwd)"
SIGN="$DIR/sign-macos-binaries.sh"
VERIFY="$DIR/verify-macos-signed.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

printf 'int main(void){return 0;}\n' >"$WORK/p.c"
cc -o "$WORK/base" "$WORK/p.c"
cat >"$WORK/taskallow.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>com.apple.security.get-task-allow</key><true/></dict></plist>
PLIST

fresh() { local f="$WORK/$1"; cp "$WORK/base" "$f"; codesign --remove-signature "$f" 2>/dev/null || true; echo "$f"; }

# expect <description> <want exit> <command...>
expect() {
  local desc="$1" want="$2" rc=0
  shift 2
  "$@" >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want)"
    FAILED=1
  fi
}

b="$(fresh good)"; "$SIGN" --adhoc "$b" >/dev/null
expect "hardened-runtime signature passes with --allow-adhoc" 0 "$VERIFY" --allow-adhoc "$b"
expect "an ad-hoc signature is refused in release mode" 1 "$VERIFY" "$b"

b="$(fresh bright)"
"$SIGN" --adhoc --identifier-prefix app.brightterm.Bright "$b" >/dev/null
expect "Bright's signing identifier passes" 0 "$VERIFY" --allow-adhoc --identifier-prefix app.brightterm.Bright "$b"
expect "a wrong Bright signing identifier is refused" 1 "$VERIFY" --allow-adhoc --identifier-prefix example.invalid.Bright "$b"

app="$WORK/Bright.app"
mkdir -p "$app/Contents/MacOS"
cp "$WORK/base" "$app/Contents/MacOS/bright"
cp "$WORK/base" "$app/Contents/MacOS/lumen-decoder-worker"
cp "$WORK/base" "$app/Contents/MacOS/lumen-holder"
cat >"$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>bright</string>
<key>CFBundleIdentifier</key><string>app.brightterm.Bright</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
"$SIGN" --adhoc --identifier-prefix app.brightterm.Bright \
  "$app/Contents/MacOS/bright" \
  "$app/Contents/MacOS/lumen-decoder-worker" \
  "$app/Contents/MacOS/lumen-holder" \
  "$app" >/dev/null
expect "the signed Bright app bundle passes" 0 "$VERIFY" --allow-adhoc --identifier-prefix app.brightterm.Bright "$app"
expect "the signed decoder identifier passes" 0 "$VERIFY" --allow-adhoc --identifier-prefix app.brightterm.Bright "$app/Contents/MacOS/lumen-decoder-worker"
expect "the signed holder identifier passes" 0 "$VERIFY" --allow-adhoc --identifier-prefix app.brightterm.Bright "$app/Contents/MacOS/lumen-holder"

b="$(fresh unsigned)"
expect "an unsigned binary is refused" 1 "$VERIFY" --allow-adhoc "$b"

b="$(fresh noruntime)"; codesign --force --sign - "$b" 2>/dev/null
expect "a signature without the hardened runtime is refused" 1 "$VERIFY" --allow-adhoc "$b"

b="$(fresh taskallow)"
codesign --force --timestamp=none --options runtime --entitlements "$WORK/taskallow.plist" --sign - "$b" 2>/dev/null
expect "get-task-allow is refused even with the hardened runtime" 1 "$VERIFY" --allow-adhoc "$b"

b="$(fresh tampered)"; "$SIGN" --adhoc "$b" >/dev/null; printf 'x' >>"$b"
expect "a modified signed binary is refused" 1 "$VERIFY" --allow-adhoc "$b"

expect "a missing binary is refused" 1 "$VERIFY" --allow-adhoc "$WORK/nope"
expect "no binary is a usage error" 2 "$VERIFY"
expect "an unknown verifier argument is a usage error" 2 "$VERIFY" --bogus

b="$(fresh nocert)"
expect "signing without the certificate refuses" 1 env -u APPLE_CERTIFICATE_P12 -u APPLE_CERTIFICATE_PASSWORD "$SIGN" "$b"
expect "a refused signing leaves the binary unsigned" 1 "$VERIFY" --allow-adhoc "$b"
expect "signing with only half the certificate settings refuses" 1 env -u APPLE_CERTIFICATE_PASSWORD APPLE_CERTIFICATE_P12=eA== "$SIGN" "$b"
expect "signing a missing binary refuses" 1 "$SIGN" --adhoc "$WORK/nope"
expect "an unknown signer argument is a usage error" 2 "$SIGN" --bogus
expect "a signer identifier without a value is a usage error" 2 "$SIGN" --identifier-prefix
expect "no binary is a signer usage error" 2 "$SIGN"

exit "$FAILED"
