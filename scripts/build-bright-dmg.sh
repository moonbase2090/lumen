#!/usr/bin/env bash
# Build the drag-to-Applications disk image for a packaged Bright.app.
#
# usage: build-bright-dmg.sh APP OUTPUT_DMG
#
# APP is the signed (and, for a release, notarized and stapled) Bright.app. The
# image holds a copy of it and a link to /Applications, and nothing else. It
# refuses to overwrite OUTPUT_DMG. The image itself still has to be signed and
# notarized afterwards: this script only builds it.
set -euo pipefail

if [[ $# -ne 2 || "$1" == -* || "$2" == -* ]]; then
  echo "usage: build-bright-dmg.sh APP OUTPUT_DMG" >&2
  exit 2
fi
app="$1"
dmg="$2"
[[ "$(uname -s)" == Darwin ]] || { echo "build-bright-dmg: macOS is required" >&2; exit 1; }
[[ -d "$app" && ! -L "$app" && "$(basename "$app")" == Bright.app ]] || {
  echo "build-bright-dmg: not a Bright.app directory: $app" >&2
  exit 1
}
[[ "$dmg" == *.dmg ]] || { echo "build-bright-dmg: the output must end in .dmg" >&2; exit 1; }
[[ ! -e "$dmg" ]] || { echo "build-bright-dmg: refusing to overwrite $dmg" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/Bright"
ditto "$app" "$work/Bright/Bright.app"
ln -s /Applications "$work/Bright/Applications"
hdiutil create -quiet -volname Bright -srcfolder "$work/Bright" -fs HFS+ -format UDZO "$dmg"
[[ -s "$dmg" ]] || { echo "build-bright-dmg: no image was written" >&2; exit 1; }
echo "build-bright-dmg: built $(basename "$dmg")"
