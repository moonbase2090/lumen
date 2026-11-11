#!/usr/bin/env bash
# Refuse a release set that cannot show a window or an image.
#
# usage: verify-release-set.sh DIR
#
# DIR holds the binaries of one release. Each check below has to hold; the first
# failure exits 1.
#   - lumen-term, lumen-decoder-worker and lumen-holder are there, are regular
#     files and are executable. The worker has to sit next to the terminal: the
#     terminal looks for it beside its own executable, and without it every
#     image stays on its alt text.
#   - lumen-term was built with the `window` feature. A build without it holds
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
for name in lumen-term lumen-decoder-worker lumen-holder; do
  [[ -f "$dir/$name" && ! -L "$dir/$name" ]] || fail "$name is missing from $dir"
  [[ -x "$dir/$name" ]] || fail "$name is not executable"
done
[[ ! -e "$dir/lumen-decoder-hostile" ]] || fail "the test-only lumen-decoder-hostile is in the set"
if grep -aq 'this build has no window' "$dir/lumen-term"; then
  fail "lumen-term was built without the window feature"
fi
echo "verify-release-set: ok (lumen-term with a window, lumen-decoder-worker beside it, lumen-holder)"
