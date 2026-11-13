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
test "$(grep -c '@VERSION@' "$plist")" -eq 0

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
