#!/usr/bin/env bash
# Build the Bright Linux release directory from binaries in TARGET_DIR.
#
# usage: package-bright-linux.sh TARGET_DIR OUTPUT_DIR [ARCH]
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 || "$1" == -* || "$2" == -* ]]; then
  echo "usage: package-bright-linux.sh TARGET_DIR OUTPUT_DIR [ARCH]" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source_dir="$1"
output_dir="$2"
arch="${3:-$(uname -m)}"
[[ "$arch" =~ ^[A-Za-z0-9._-]+$ ]] || {
  echo "package-bright-linux: ARCH may contain only letters, digits, dot, underscore and hyphen" >&2
  exit 2
}
package="$output_dir/bright-linux-$arch"
bin="$package/bin"

for name in lumen-term lumen-decoder-worker lumen-holder; do
  [[ -f "$source_dir/$name" && ! -L "$source_dir/$name" ]] || {
    echo "package-bright-linux: missing regular file: $source_dir/$name" >&2
    exit 1
  }
done

rm -rf "$package"
mkdir -p "$bin" "$package/share/applications"
install -m 755 "$source_dir/lumen-term" "$bin/bright"
install -m 755 "$source_dir/lumen-decoder-worker" "$bin/lumen-decoder-worker"
install -m 755 "$source_dir/lumen-holder" "$bin/lumen-holder"
install -m 644 "$ROOT/packaging/linux/app.brightterm.Bright.desktop" \
  "$package/share/applications/app.brightterm.Bright.desktop"
echo "$package"
