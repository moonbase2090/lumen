#!/usr/bin/env bash
# Package-shape tests use stand-in executables. Runtime proof uses real builds.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
source_dir="$WORK/bin"
mkdir -p "$source_dir"
for name in lumen-term lumen-decoder-worker lumen-holder; do
  printf '#!/bin/sh\nexit 0\n' >"$source_dir/$name"
  chmod 755 "$source_dir/$name"
done

mac_root="$WORK/mac"
"$ROOT/scripts/package-bright-macos.sh" "$source_dir" "$mac_root" >/dev/null
app="$mac_root/Bright.app"
for name in bright lumen-decoder-worker lumen-holder; do
  test -x "$app/Contents/MacOS/$name"
  test ! -L "$app/Contents/MacOS/$name"
done
plist="$app/Contents/Info.plist"
grep -q '<string>Bright</string>' "$plist"
grep -q '<string>bright</string>' "$plist"
grep -q '<string>app.brightterm.Bright</string>' "$plist"
python3 - "$plist" <<'PYTHON'
import plistlib
import re
import sys

with open(sys.argv[1], "rb") as bundle:
    info = plistlib.load(bundle)
assert re.fullmatch(r"[0-9]+(?:\.[0-9]+){2}", info["CFBundleShortVersionString"])
assert re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,2}", info["CFBundleVersion"])
PYTHON

if [[ "$(uname -s)" == Darwin ]]; then
  # Mach-O stand-ins: the linker signs them ad hoc, as it signs the release
  # binaries, and a bundle left with only that signature does not verify.
  macho="$WORK/macho"
  mkdir -p "$macho"
  printf 'int main(void) { return 0; }\n' >"$WORK/main.c"
  for name in lumen-term lumen-decoder-worker lumen-holder; do
    cc -o "$macho/$name" "$WORK/main.c"
  done
  "$ROOT/scripts/package-bright-macos.sh" "$macho" "$WORK/signed" >/dev/null 2>&1
  signed="$WORK/signed/Bright.app"
  codesign --verify --deep --strict "$signed" || {
    echo "test-bright-packaging: the packaged bundle's signature does not verify" >&2
    exit 1
  }
  identifier() { codesign -dv "$1" 2>&1 | sed -n 's/^Identifier=//p'; }
  test "$(identifier "$signed")" = app.brightterm.Bright
  test "$(identifier "$signed/Contents/MacOS/lumen-decoder-worker")" = app.brightterm.Bright.decoder
  test "$(identifier "$signed/Contents/MacOS/lumen-holder")" = app.brightterm.Bright.holder
  # The signature seals Info.plist: a change after signing must fail the check.
  printf '\n' >>"$signed/Contents/Info.plist"
  if codesign --verify --deep --strict "$signed" 2>/dev/null; then
    echo "test-bright-packaging: a bundle changed after signing still verified" >&2
    exit 1
  fi
fi

linux_root="$WORK/linux"
"$ROOT/scripts/package-bright-linux.sh" "$source_dir" "$linux_root" test-arch >/dev/null
package="$linux_root/bright-linux-test-arch"
for name in bright lumen-decoder-worker lumen-holder; do
  test -x "$package/bin/$name"
  test ! -L "$package/bin/$name"
done
desktop="$package/share/applications/app.brightterm.Bright.desktop"
grep -qx 'Name=Bright' "$desktop"
grep -qx 'Comment=The Lumen reference terminal' "$desktop"
grep -qx 'Exec=bright' "$desktop"
grep -qx 'StartupWMClass=Bright' "$desktop"

if "$ROOT/scripts/package-bright-macos.sh" "$WORK/missing" "$WORK/nope" >/dev/null 2>&1; then
  echo "test-bright-packaging: missing binaries passed" >&2
  exit 1
fi
if "$ROOT/scripts/package-bright-linux.sh" "$source_dir" "$WORK/nope" ../outside >/dev/null 2>&1; then
  echo "test-bright-packaging: unsafe architecture passed" >&2
  exit 1
fi
echo "test-bright-packaging: ok"
