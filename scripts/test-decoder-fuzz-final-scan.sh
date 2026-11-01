#!/usr/bin/env bash
# A dry run that injects one input during the final slice's sync. The scan after
# the slice loop must record it before decoder-fuzz.sh writes the ledger.
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
    escape) printf '{"pass":true}\n' ;;
    format)
      test -s "$3"
      printf '{"format":"%s"}\n' "$1"
      ;;
    ledger) printf '{"revision":"fz-1"}\n' ;;
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

sync_command='if [ ! -e "$OUT/final-sync-injected" ]; then mkdir -p "$OUT/png/artifacts"; printf input > "$OUT/png/artifacts/timeout-final-scan"; : > "$OUT/final-sync-injected"; fi'
"$RUN" --out "$T/out" --workers 1 --slice-seconds 1 --slices 1 \
  --formats png --seeds "$T/seeds" --sync-cmd "$sync_command" > "$T/run.log" 2>&1

test "$(wc -l < "$T/out/png/crashes.jsonl" | tr -d ' ')" -eq 1
grep -q '"kind": "hang"' "$T/out/png/crashes.jsonl"
grep -qxF timeout-final-scan "$T/out/png/artifacts-seen.txt"
echo 'final artifact scan dry run passed'
