#!/usr/bin/env bash
# Tests scripts/check-repo-links.sh on small trees: a link to a file that exists
# passes, a link to one that does not exist fails and names it, a link to
# another repository is left to the network check, and a directory link works.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
fail=0
base='https://github.com/moonbase2090/lumen'

mk() { rm -rf "$T/t"; mkdir -p "$T/t/docs" "$T/t/site/content"; : > "$T/t/docs/real.md"; }

mk
printf '[a](%s/blob/main/docs/real.md#section) and [b](%s/tree/main/docs)\n' "$base" "$base" > "$T/t/site/content/p.md"
if out="$("$ROOT/scripts/check-repo-links.sh" "$T/t" 2>&1)"; then echo "ok    links to files and folders in the tree pass"
else echo "FAIL  links that resolve were rejected"; echo "$out"; fail=1; fi

mk
printf '[a](%s/blob/main/docs/gone.md)\n' "$base" > "$T/t/site/content/p.md"
out="$("$ROOT/scripts/check-repo-links.sh" "$T/t" 2>&1)"; rc=$?
if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q 'docs/gone.md'; then echo "ok    a link to a missing file fails and names it"
else echo "FAIL  a link to a missing file was not caught (rc=$rc)"; echo "$out"; fail=1; fi

mk
printf '[a](https://github.com/someone/else/blob/main/nothing.md)\n' > "$T/t/site/content/p.md"
if out="$("$ROOT/scripts/check-repo-links.sh" "$T/t" 2>&1)"; then echo "ok    a link to another repository is not checked here"
else echo "FAIL  a link to another repository was rejected"; echo "$out"; fail=1; fi

exit "$fail"
