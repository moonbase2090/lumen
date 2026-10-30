#!/usr/bin/env bash
# Scan public text (tracked files and, optionally, the built site) for
# disallowed patterns: home paths, machine names, private IPs, key-like
# strings, and emails other than the allowed ones. Adapted from the
# release-text check used in other moonbase2090 repos.
# Prints only path:line on failure; never prints the matched text.
# Extra private terms (one regex per line) can be supplied via the
# PUBLIC_TEXT_DENYLIST env var or a denylist file; they are never logged.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# The one exact, case-sensitive allowlist entry: the company's legal name. It
# is removed from each line before the personal-identifier patterns run, so an
# identifier beside it on the same line still fails.
ALLOWED_NAME='Moonbase 2090 LLC'
ALLOWED_EMAIL_RE='^(322824348\+mb2090@users\.noreply\.github\.com|[A-Za-z0-9._%+-]+@lumenprotocol\.org)$'
DENYLIST_FILE="${PUBLIC_TEXT_DENYLIST_FILE:-$HOME/.config/moonbase/release-denylist.txt}"
EMAIL_PATTERN='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'

GENERIC_PATTERNS=(
  'DESKTOP-[A-Z0-9]+'
  '[A-Za-z0-9-]+\.local\b'
  '/Users/[A-Za-z]'
  '/home/[a-z]'
  'C:\\Users'
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  '(ghp|gho|ghs|ghu|ghr|github_pat)_[A-Za-z0-9_]{20,}'
  '\bAKIA[0-9A-Z]{16}\b'
  '\bxox[abposr]-[A-Za-z0-9-]{10,}'
  '\bsk-[A-Za-z0-9_-]{20,}'
  '\bAIza[0-9A-Za-z_-]{35}\b'
  '\b(10\.[0-9]+\.[0-9]+\.[0-9]+|192\.168\.[0-9]+\.[0-9]+|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]+\.[0-9]+|100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.[0-9]+\.[0-9]+)\b'
  "\bowner'?s?[[:space:]]+(decision|acceptance|sign-?off)\b"
  "\b(on|by)[[:space:]]+the[[:space:]]+owner'?s?\b"
  '\baccepted[[:space:]]+by[[:space:]]+the[[:space:]]+owner\b'
)

TMP_PATTERNS="$(mktemp)"
TMP_FILES="$(mktemp)"
TMP_SAN="$(mktemp)"
trap 'rm -f "$TMP_PATTERNS" "$TMP_FILES" "$TMP_SAN"' EXIT

{
  printf '%s\n' "${GENERIC_PATTERNS[@]}"
  if [[ -f "$DENYLIST_FILE" ]]; then
    grep -v '^[[:space:]]*$' "$DENYLIST_FILE" | grep -v '^[[:space:]]*#' || true
  fi
  if [[ -n "${PUBLIC_TEXT_DENYLIST:-}" ]]; then
    printf '%s\n' "$PUBLIC_TEXT_DENYLIST" | grep -v '^[[:space:]]*$' || true
  fi
} >"$TMP_PATTERNS"

line_no=0
while IFS= read -r pat || [[ -n "$pat" ]]; do
  line_no=$((line_no + 1))
  rc=0
  printf '\n' | grep -qiE -- "$pat" >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq 2 ]]; then
    echo "check-public-text: invalid regex at patterns:${line_no}" >&2
    exit 2
  fi
done <"$TMP_PATTERNS"

# Files to scan: tracked files (minus this script, which holds the
# patterns, and verbatim LICENSE texts) plus every file under any directory passed as an argument.
git ls-files -z | tr '\0' '\n' | grep -vE '^scripts/check-public-text\.sh$|(^|/)LICENSE[^/]*$' >"$TMP_FILES"
for dir in "$@"; do
  [[ -d "$dir" ]] || { echo "check-public-text: no such directory: $dir" >&2; exit 2; }
  # Pagefind's vendored, minified JS bundles are third-party code; the
  # generated index and all site pages are still scanned.
  find "$dir" -type f ! -path '*/pagefind/*.js' >>"$TMP_FILES"
done

HITS=0
while IFS= read -r file; do
  [[ -f "$file" ]] || continue
  # Skip binary files (fonts, compressed search index chunks).
  grep -Iq . "$file" 2>/dev/null || continue
  # Scan a copy with the exact allowlisted name removed, line numbers kept.
  sed "s/${ALLOWED_NAME}/ /g" <"$file" >"$TMP_SAN"
  while IFS= read -r ln; do
    [[ -n "$ln" ]] && { echo "$file:$ln"; HITS=1; }
  done < <(grep -niE -f "$TMP_PATTERNS" -- "$TMP_SAN" 2>/dev/null | cut -d: -f1 | sort -un)
  while IFS=: read -r ln content; do
    [[ -z "$ln" ]] && continue
    while IFS= read -r email; do
      [[ -z "$email" ]] && continue
      if ! [[ "$email" =~ $ALLOWED_EMAIL_RE ]]; then
        echo "$file:$ln"; HITS=1
      fi
    done < <(grep -oE "$EMAIL_PATTERN" <<<"$content" || true)
  done < <(grep -nE "$EMAIL_PATTERN" -- "$TMP_SAN" 2>/dev/null || true)
done <"$TMP_FILES"

if [[ "$HITS" -ne 0 ]]; then
  echo "check-public-text: disallowed text detected (see path:line above)" >&2
  exit 1
fi
echo "check-public-text: ok"
