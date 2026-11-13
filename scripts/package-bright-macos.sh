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
if text.count("@VERSION@") != 2:
    raise SystemExit("package-bright-macos: Info.plist template must contain two version fields")
destination.write_text(text.replace("@VERSION@", version), encoding="utf-8")
PY
echo "$app"
