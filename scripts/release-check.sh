#!/usr/bin/env bash
# Release gate. Refuses (exit 1) unless every check below holds; it never
# publishes, tags or waives anything, and a missing input is a refusal, not a
# skip. It prints reasons and paths only, never the text a scanner matched.
#
#   1. The personal-identifier release scan passes (scripts/check-public-text.sh).
#   2. The decoder fuzz evidence exists: the ledger file is present, readable
#      JSON, and names both build identities and at least one format.
#   3. No format in the ledger records a crash. A crash is open until a patched
#      build, which has a new identity, has its own ledger (release criteria,
#      rule 2).
#   4. The ledger's decoder and image-path build identities equal the identities
#      of this tree, computed with the flags the campaign recorded. A changed
#      identity invalidates the earlier pass (rule 5).
#   5. The decoder judge (`lumen-evidence --decoder`) exits 0: the thresholds of
#      docs/release-criteria.md hold for the ledger.
#
# Usage: scripts/release-check.sh [--ledger FILE] [--fuzz-flags-file FILE]
#   --ledger            default release/decoder-ledger.json
#   --fuzz-flags-file   the exact flags string the campaign passed to the
#                       identity tool (decoder-fuzz.sh FUZZ_FLAGS); the ledger
#                       does not carry it. Default release/decoder-fuzz-flags.txt
#
# Commands under test can be replaced (the tests do): RELEASE_CHECK_PII,
# RELEASE_CHECK_JUDGE, RELEASE_CHECK_IDENTITY name a command each.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LEDGER="release/decoder-ledger.json"
FLAGS_FILE="release/decoder-fuzz-flags.txt"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --ledger) LEDGER="${2:?--ledger needs a file}"; shift 2 ;;
    --fuzz-flags-file) FLAGS_FILE="${2:?--fuzz-flags-file needs a file}"; shift 2 ;;
    *) echo "release-check: unknown argument $1" >&2; exit 2 ;;
  esac
done

PII="${RELEASE_CHECK_PII:-scripts/check-public-text.sh}"
JUDGE="${RELEASE_CHECK_JUDGE:-cargo run -q --locked -p lumen-evidence --}"
IDENTITY="${RELEASE_CHECK_IDENTITY:-cargo run -q --locked -p lumen-decoder-tools -- identity}"

PROBLEMS=0
refuse() {
  echo "REFUSED: $1" >&2
  PROBLEMS=$((PROBLEMS + 1))
}

# 1. Personal-identifier scan. Its own output is paths only.
pii_rc=0
$PII >&2 || pii_rc=$?
[[ "$pii_rc" -eq 0 ]] || refuse "the personal-identifier scan did not pass (exit $pii_rc)"

# 2. Evidence present and well formed.
ledger_ok=1
if [[ ! -f "$LEDGER" ]]; then
  refuse "no decoder fuzz ledger at $LEDGER"
  ledger_ok=0
elif ! python3 -I - "$LEDGER" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
ok = (isinstance(d, dict)
      and isinstance(d.get("decoder_build"), str) and d["decoder_build"]
      and isinstance(d.get("image_path_build"), str) and d["image_path_build"]
      and isinstance(d.get("formats"), list) and d["formats"]
      and all(isinstance(f, dict) and isinstance(f.get("crashes"), list) for f in d["formats"]))
sys.exit(0 if ok else 1)
PY
then
  refuse "the ledger at $LEDGER is not readable JSON with both build identities and a crash list for each format"
  ledger_ok=0
fi

# 3 and 4 need a usable ledger.
if [[ "$ledger_ok" -eq 1 ]]; then
  # 3. Open crash evidence.
  crashes="$(python3 -I - "$LEDGER" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(sum(len(f["crashes"]) for f in d["formats"]))
PY
)"
  [[ "$crashes" -eq 0 ]] || refuse "the ledger records $crashes crash(es); a crash blocks release until a patched build has a clean ledger"

  # 4. Identity of this tree against the ledger.
  if [[ ! -s "$FLAGS_FILE" ]]; then
    refuse "no fuzz flags string at $FLAGS_FILE, so the build identity cannot be recomputed"
  else
    flags="$(cat "$FLAGS_FILE")"
    for kind in decoder image-path; do
      want="$(python3 -I - "$LEDGER" "$kind" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(d["decoder_build" if sys.argv[2] == "decoder" else "image_path_build"])
PY
)"
      id_rc=0
      have="$($IDENTITY "$ROOT" "$flags" "$kind" 2>/dev/null)" || id_rc=$?
      if [[ "$id_rc" -ne 0 || -z "$have" ]]; then
        refuse "could not compute the $kind build identity"
      elif [[ "$have" != "$want" ]]; then
        refuse "the $kind build identity of this tree differs from the ledger's; the earlier fuzz pass does not cover it"
      fi
    done
  fi
fi

# 5. The judge. Its exit status is the only thing read from it: 0 is a pass,
# 1 a failed gate, 3 nothing measured, 2 a usage or load error.
if [[ "$ledger_ok" -eq 1 ]]; then
  out="$(mktemp -d)"
  commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  judge_rc=0
  $JUDGE --decoder --ledger "$LEDGER" --commit "$commit" --out "$out" >/dev/null 2>&1 || judge_rc=$?
  rm -rf "$out"
  [[ "$judge_rc" -eq 0 ]] || refuse "the decoder judge did not pass the ledger (exit $judge_rc)"
fi

if [[ "$PROBLEMS" -ne 0 ]]; then
  echo "release-check: refused ($PROBLEMS problem(s)); nothing was released" >&2
  exit 1
fi
echo "release-check: ok (this checks the gate only; it releases nothing)"
