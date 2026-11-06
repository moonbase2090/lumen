#!/usr/bin/env bash
# Dry-run the driver with more than ARG_MAX of crash history. The ledger tool
# must receive file paths, not the serialized history as process arguments.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/scripts/decoder-fuzz.sh"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT

mkdir -p "$T/bin" "$T/seeds/png"
printf seed > "$T/seeds/png/seed"

cat > "$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = run ]; then
  while [ "${1:-}" != -- ]; do shift; done
  shift
  command="$1"; shift
  case "$command" in
    identity) printf '%064d\n' 1 ;;
    gate-status) printf '{"cpu_hours":0,"holds":false}\n' ;;
    escape) printf '{"revision":"se-1","passed":1,"total":1}\n' ;;
    ledger-files)
      test "$5" = png
      test -s "$6"
      test "$(wc -c < "$7" | tr -d ' ')" -gt "$(getconf ARG_MAX)"
      printf '{"revision":"fz-1"}\n'
      ;;
    annotate) cat "$1" ;;
    *) echo "unexpected decoder-tools command: $command" >&2; exit 2 ;;
  esac
  exit 0
fi

if [ "${1:-}" = build ]; then
  mkdir -p "$FAKE_REPO/target/release"
  : > "$FAKE_REPO/target/release/lumen-decoder-worker"
  exit 0
fi

if [[ "${1:-}" = +* ]] && [ "${2:-}" = fuzz ]; then
  [ "${3:-}" = build ] || printf '#1: cov: 1 ft: 1 corp: 1/1b exec/s: 1\n'
  exit 0
fi

if [ "${1:-}" = test ]; then
  echo 'test result: ok'
  exit 0
fi

echo "unexpected cargo command: $*" >&2
exit 2
SH
chmod +x "$T/bin/cargo"

export FAKE_REPO="$ROOT"
export PATH="$T/bin:$PATH"
export ARTIFACT_POLL=60

cat > "$T/sync" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ -e "$OUT/large-history" ] && exit 0
python3 - "$OUT/png/crashes.jsonl" <<'PY'
import os, sys
line = '{"at_cpu_hour":1.0,"kind":"sanitizer","artifact":"' + 'a' * 64 + '"}\n'
with open(sys.argv[1], "w") as out:
    while out.tell() <= os.sysconf("SC_ARG_MAX"):
        out.write(line)
PY
: > "$OUT/large-history"
SH
chmod +x "$T/sync"

"$RUN" --out "$T/out" --workers 64 --slice-seconds 1 --slices 1 \
  --formats png --seeds "$T/seeds" --sync-cmd "$T/sync" > "$T/run.log" 2>&1

test -s "$T/out/ledger.json"
echo 'large ledger history stayed off argv'
