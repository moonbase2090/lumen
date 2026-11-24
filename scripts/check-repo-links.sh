#!/usr/bin/env bash
# Checks the links this repository makes to its own files on github.com
# (https://github.com/moonbase2090/lumen/blob/main/<path> and .../tree/main/<path>)
# against the working tree, so the link check does not depend on github.com
# answering: those links are excluded from the network check in
# .github/workflows/links.yml and verified here instead. A link whose target
# is not in the tree fails.
#
#   scripts/check-repo-links.sh [root]      root defaults to the repository root
set -uo pipefail
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT" || exit 2
pattern='https://github\.com/moonbase2090/lumen/(blob|tree)/main/[^)"'"'"' <>#?]+'
bad=0
count=0
while IFS= read -r hit; do
  file="${hit%%:*}"
  rest="${hit#*:}"
  url="$(printf '%s' "$rest" | grep -oE "$pattern" | head -1)"
  path="${url#*/main/}"
  count=$((count + 1))
  if [ ! -e "$path" ]; then
    echo "FAIL  $file links to $path, which is not in the tree"
    bad=1
  fi
done < <(grep -rEoH --include='*.md' --include='*.html' --include='*.toml' --include='*.yml' \
  --exclude-dir=target --exclude-dir=node_modules --exclude-dir=.git --exclude-dir=public \
  -n "$pattern" . 2>/dev/null | sed -E 's|^\./||; s|^([^:]+):[0-9]+:|\1:|')
[ "$bad" -eq 0 ] && echo "ok    $count in-repo github.com links resolve in the tree"
exit "$bad"
