#!/usr/bin/env bash
# Refuse a release set that cannot show a window or an image.
#
# usage: verify-release-set.sh DIR
#
# DIR holds Bright.app. Each check below has to hold; the first failure exits 1.
#   - bright, lumen-decoder-worker and lumen-holder are regular executables in
#     Contents/MacOS. The worker has to sit next to Bright: the terminal looks
#     for it beside its own executable, and without it every image stays on its
#     alt text.
#   - bright was built with the `window` feature. A build without it holds
#     the refusal text "this build has no window" and cannot open a window.
#   - the test-only worker lumen-decoder-hostile is not in the set.
set -euo pipefail

if [[ $# -ne 1 || "$1" == -* ]]; then
  echo "usage: verify-release-set.sh DIR" >&2
  exit 2
fi
dir="$1"
fail() { echo "verify-release-set: $1" >&2; exit 1; }

[[ -d "$dir" ]] || fail "not a directory: $dir"
app="$dir/Bright.app"
contents="$app/Contents"
macos="$contents/MacOS"
plist="$contents/Info.plist"
[[ -d "$app" && ! -L "$app" ]] || fail "Bright.app is missing from $dir"
[[ -f "$plist" && ! -L "$plist" ]] || fail "Info.plist is missing from Bright.app"
plist_value() {
  python3 - "$plist" "$1" <<'PY'
import plistlib
import sys
with open(sys.argv[1], "rb") as source:
    value = plistlib.load(source).get(sys.argv[2], "")
print(value if isinstance(value, str) else "")
PY
}
[[ "$(plist_value CFBundleDisplayName)" == "Bright" ]] || fail "CFBundleDisplayName is not Bright"
[[ "$(plist_value CFBundleName)" == "Bright" ]] || fail "CFBundleName is not Bright"
[[ "$(plist_value CFBundleExecutable)" == "bright" ]] || fail "CFBundleExecutable is not bright"
[[ "$(plist_value CFBundleIdentifier)" == "app.brightterm.Bright" ]] || fail "CFBundleIdentifier is not app.brightterm.Bright"
for name in bright lumen-decoder-worker lumen-holder; do
  [[ -f "$macos/$name" && ! -L "$macos/$name" ]] || fail "$name is missing from Bright.app"
  [[ -x "$macos/$name" ]] || fail "$name is not executable"
done
[[ ! -e "$macos/lumen-decoder-hostile" ]] || fail "the test-only lumen-decoder-hostile is in Bright.app"
if grep -aq 'this build has no window' "$macos/bright"; then
  fail "bright was built without the window feature"
fi
echo "verify-release-set: ok (Bright.app with a window, lumen-decoder-worker beside it, lumen-holder)"
