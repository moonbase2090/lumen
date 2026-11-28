#!/usr/bin/env bash
# Write the notes for a preview release of Bright.
#
# usage: release-notes-preview.sh TAG DIST_DIR OUTPUT_FILE
#
# TAG is like v0.1.0-preview.1. DIST_DIR holds the disk image and its
# SHA256SUMS. The text is fixed apart from the tag and the checksums, so a tag
# message or an artifact name cannot add private text to it; the caller still
# runs scripts/check-public-text.sh over the result.
set -euo pipefail

if [[ $# -ne 3 || "$1" == -* ]]; then
  echo "usage: release-notes-preview.sh TAG DIST_DIR OUTPUT_FILE" >&2
  exit 2
fi
tag="$1"
dist="$2"
out="$3"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-preview\.[0-9]+$ ]] || {
  echo "release-notes-preview: not a preview tag: $tag" >&2
  exit 1
}
[[ -f "$dist/SHA256SUMS" ]] || { echo "release-notes-preview: no SHA256SUMS in $dist" >&2; exit 1; }
grep -Eq '^[0-9a-f]{64}  Bright-[0-9A-Za-z.+-]+\.dmg$' "$dist/SHA256SUMS" || {
  echo "release-notes-preview: SHA256SUMS lists no Bright disk image" >&2
  exit 1
}

{
  printf '# Bright %s\n\n' "${tag#v}"
  printf 'A preview of Bright, the Lumen terminal, for macOS on Apple Silicon.\n\n'
  printf 'It is a preview: expect rough edges, and do not rely on it yet.\n\n'
  printf 'The disk image is signed with a Developer ID, notarized by Apple and stapled, so Gatekeeper opens it without a warning. Open it and drag Bright to Applications.\n\n'
  printf '## Checksums\n\n```\n'
  cat "$dist/SHA256SUMS"
  printf '```\n'
} >"$out"
echo "release-notes-preview: wrote $out"
