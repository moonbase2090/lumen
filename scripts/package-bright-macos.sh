#!/usr/bin/env bash
# Build Bright.app from the three release binaries in TARGET_DIR.
#
# usage: package-bright-macos.sh TARGET_DIR OUTPUT_DIR
set -euo pipefail

if [[ $# -ne 2 || "$1" == -* || "$2" == -* ]]; then
  echo "usage: package-bright-macos.sh TARGET_DIR OUTPUT_DIR" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source_dir="$1"
output_dir="$2"
app="$output_dir/Bright.app"
macos="$app/Contents/MacOS"

for name in lumen-term lumen-decoder-worker lumen-holder; do
  [[ -f "$source_dir/$name" && ! -L "$source_dir/$name" ]] || {
    echo "package-bright-macos: missing regular file: $source_dir/$name" >&2
    exit 1
  }
done

version="$(cargo metadata --format-version 1 --no-deps --manifest-path "$ROOT/Cargo.toml" \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next(p["version"] for p in d["packages"] if p["name"] == "lumen-term"))')"
[[ -n "$version" ]] || { echo "package-bright-macos: no package version" >&2; exit 1; }

rm -rf "$app"
mkdir -p "$macos"
install -m 755 "$source_dir/lumen-term" "$macos/bright"
install -m 755 "$source_dir/lumen-decoder-worker" "$macos/lumen-decoder-worker"
install -m 755 "$source_dir/lumen-holder" "$macos/lumen-holder"
python3 - "$ROOT/packaging/macos/Info.plist.in" "$app/Contents/Info.plist" "$version" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1])
destination = Path(sys.argv[2])
version = sys.argv[3]
text = source.read_text(encoding="utf-8")
if text.count("@VERSION@") != 1:
    raise SystemExit("package-bright-macos: Info.plist template must contain one release version field")
destination.write_text(text.replace("@VERSION@", version.partition("+")[0].partition("-")[0]), encoding="utf-8")
PY

# Signing is the last step: the bundle's signature seals Info.plist and the
# executables, so nothing may change after it. The linker's ad-hoc signature on
# the main executable seals no resources, and macOS rejects that bundle. Sign
# the helpers first, then the bundle, ad hoc and with the identifiers the
# release signing uses. A release re-signs the same paths with its Developer ID.
if [[ "$(uname -s)" == Darwin ]]; then
  sign() { codesign --force --timestamp=none --identifier "$1" --sign - "$2"; }
  sign app.brightterm.Bright.decoder "$macos/lumen-decoder-worker"
  sign app.brightterm.Bright.holder "$macos/lumen-holder"
  sign app.brightterm.Bright "$app"
  codesign --verify --deep --strict "$app" || {
    echo "package-bright-macos: the signed bundle does not verify" >&2
    exit 1
  }
else
  echo "package-bright-macos: not signed (codesign needs macOS)" >&2
fi
echo "$app"
