#!/usr/bin/env bash
# Starts (or resumes) the decoder gate run on a fresh Debian or Ubuntu VM.
# Meant to be the VM's start-up script or to be run by hand; it is safe to run
# again on the same VM or on a replacement VM, because the campaign resumes from
# whatever RESTORE_CMD brings back.
#
#   COMMIT=<full commit> [options below] scripts/decoder-fuzz-vm.sh
#
# Environment:
#   COMMIT            required: the full commit to fuzz (the merge commit)
#   REPO              default https://github.com/moonbase2090/lumen
#   NIGHTLY           default nightly-2026-10-06 (pin it: the toolchain is part
#                     of the build identity, and a resumed run must match)
#   TARGET_CPU_HOURS  default 1200 per format
#   SLICE_SECONDS     default 900 (a VM that vanishes loses at most one slice)
#   WORKERS           default: the number of CPUs
#   RESTORE_CMD, SYNC_CMD, FINISH_CMD   see scripts/decoder-fuzz.sh; OUT is set
#   WORKDIR           default $HOME/lumen-gate
#
# Example for a one-time spot VM with an instance role that can write to a bucket:
#   RESTORE_CMD='aws s3 sync s3://BUCKET/PREFIX/ "$OUT/"'
#   SYNC_CMD='aws s3 sync "$OUT/" s3://BUCKET/PREFIX/ --only-show-errors'
#   FINISH_CMD='sudo shutdown -h now'    # with the instance set to terminate on shutdown
set -euo pipefail

: "${COMMIT:?COMMIT is required (the full commit to fuzz)}"
REPO="${REPO:-https://github.com/moonbase2090/lumen}"
NIGHTLY="${NIGHTLY:-nightly-2026-10-06}"
TARGET_CPU_HOURS="${TARGET_CPU_HOURS:-1200}"
SLICE_SECONDS="${SLICE_SECONDS:-900}"
WORKDIR="${WORKDIR:-$HOME/lumen-gate}"
WORKERS="${WORKERS:-$(getconf _NPROCESSORS_ONLN)}"
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"

if command -v apt-get >/dev/null 2>&1; then
  $SUDO apt-get update -qq
  $SUDO DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential git curl python3 rsync pkg-config procps >/dev/null
fi
if ! command -v rustup >/dev/null 2>&1; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
rustup toolchain install stable --profile minimal >/dev/null
rustup toolchain install "$NIGHTLY" --profile minimal >/dev/null
command -v cargo-fuzz >/dev/null 2>&1 || cargo +"$NIGHTLY" install cargo-fuzz --locked

mkdir -p "$WORKDIR"
[ -d "$WORKDIR/lumen/.git" ] || git clone -q "$REPO" "$WORKDIR/lumen"
git -C "$WORKDIR/lumen" fetch -q origin
git -C "$WORKDIR/lumen" checkout -q "$COMMIT"
[ "$(git -C "$WORKDIR/lumen" rev-parse HEAD)" = "$COMMIT" ] || { echo "checked out $(git -C "$WORKDIR/lumen" rev-parse HEAD), not $COMMIT" >&2; exit 2; }

args=(--out "$HOME/fz" --workers "$WORKERS" --slice-seconds "$SLICE_SECONDS" --target-cpu-hours "$TARGET_CPU_HOURS")
[ -z "${RESTORE_CMD:-}" ] || args+=(--restore-cmd "$RESTORE_CMD")
[ -z "${SYNC_CMD:-}" ] || args+=(--sync-cmd "$SYNC_CMD")
[ -z "${FINISH_CMD:-}" ] || args+=(--finish-cmd "$FINISH_CMD")

cd "$WORKDIR/lumen"
echo "gate run: commit $COMMIT, $WORKERS workers, ${TARGET_CPU_HOURS} CPU-hours per format, ${SLICE_SECONDS}s slices" >&2
NIGHTLY="$NIGHTLY" exec scripts/decoder-fuzz.sh "${args[@]}"
