#!/usr/bin/env bash
# Runs Miri on zune-jpeg's unsafe code: its own tests at the commit that
# published version the decoder is pinned to, and the decoder's JPEG seeds. Needs a nightly with Miri and
# rust-src. The x86_64 target runs on any host (Miri interprets it); Miri
# reports no AVX2 unless the feature is compiled in, so it is forced on, or the
# AVX2 code would not run at all.
#
#   scripts/decoder-miri.sh WORKDIR [--compare RECORD]
#
# WORKDIR is created; the run takes about 10 minutes. It writes
# WORKDIR/miri-record.json: for every run its command, status, the Miri result
# lines and, if it failed, the first error line; plus the toolchain, the host,
# the date and both commits. `--compare RECORD` then checks the fresh record
# against a committed one (status and result lines, timings ignored) and exits
# 1 if they differ.
set -euo pipefail
NIGHTLY="${NIGHTLY:-nightly}"
WORK="${1:?usage: decoder-miri.sh WORKDIR [--compare RECORD]}"
COMPARE=""
if [ "${2:-}" = "--compare" ]; then COMPARE="${3:?--compare needs a record}"; fi
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UPSTREAM=https://github.com/etemesi254/zune-image
# The upstream commit whose crates/zune-jpeg/Cargo.toml says the version the
# decoder pins (crates/lumen-decoder/Cargo.toml). The published package differs
# from it by at most a line; the script checks and records that.
COMMIT="${ZUNE_COMMIT:-c1c7f49776ee32928ed329f4436ae63a595b6283}"   # version = "0.5.16-rc3"
X86=x86_64-unknown-linux-gnu
AVX2="-C target-feature=+avx2,+sse4.1"
export MIRIFLAGS="-Zmiri-disable-isolation"

mkdir -p "$WORK"; WORK="$(cd "$WORK" && pwd)"
[ -d "$WORK/zune-image" ] || git clone -q --filter=blob:none "$UPSTREAM" "$WORK/zune-image"
git -C "$WORK/zune-image" checkout -q "$COMMIT"
PIN="$(sed -n 's/^zune-jpeg = "=\(.*\)"/\1/p' "$ROOT/crates/lumen-decoder/Cargo.toml")"
UPSTREAM_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$WORK/zune-image/crates/zune-jpeg/Cargo.toml" | head -1)"
if [ "$PIN" != "$UPSTREAM_VERSION" ]; then
  echo "the decoder pins zune-jpeg $PIN but commit $COMMIT is $UPSTREAM_VERSION; set ZUNE_COMMIT" >&2; exit 2
fi
PKG="$(ls -d "$HOME"/.cargo/registry/src/*/zune-jpeg-"$PIN" 2>/dev/null | head -1)"
if [ -n "$PKG" ]; then
  diff -r "$WORK/zune-image/crates/zune-jpeg/src" "$PKG/src" > "$WORK/package-vs-upstream.diff" 2>&1 || true
  echo "published package vs upstream commit: $(grep -c '^[<>]' "$WORK/package-vs-upstream.diff") differing lines" >&2
fi

run() { # name, dir, args...  (TIMEBOX=SECONDS limits the run)
  name="$1"; dir="$2"; shift 2
  box=()
  [ -z "${TIMEBOX:-}" ] || box=(python3 "$ROOT/scripts/decoder-timebox.py" "$TIMEBOX")
  rc=0
  (cd "$dir" && "${box[@]+"${box[@]}"}" "$@" > "$WORK/$name.log" 2>&1) || rc=$?
  if [ "$rc" -eq 0 ]; then st=ok; elif [ "$rc" -eq 124 ] && [ -n "${TIMEBOX:-}" ]; then st=TIMEOUT; else st=FAILED; fi
  printf '%-44s %s  (%s)\n' "$name" "$st" "$(grep -E '^test result' "$WORK/$name.log" | tr '\n' ' ' | cut -c1-110)"
  echo "$name	$st	${TIMEBOX:+timebox ${TIMEBOX}s: }$*" >> "$WORK/runs.tsv"
}
: > "$WORK/runs.tsv"

# zune-jpeg's own unit and integration tests, AVX2 forced on.
# One upstream test, set_options_refreshes_cached_dispatch, compares the addresses
# of functions cast to integers; Miri does not keep a function's address the same
# across casts, so it fails under Miri whatever the code does. It is skipped in
# the runs below and run on its own after them, so its failure is on record with
# the reason and not hidden.
SKIP="--skip set_options_refreshes_cached_dispatch"
run upstream-x86-avx2-unit  "$WORK/zune-image/crates/zune-jpeg" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test --lib   --target $X86 -- $SKIP
# The integration tests one file at a time, each with a time box: Miri interprets
# the code far more slowly than it runs, and one upstream test (in app14.rs) ran
# for over four hours without finishing. A file that does not finish is recorded
# as TIMEOUT, which says nothing about the code either way.
for t in $(ls "$WORK/zune-image/crates/zune-jpeg/tests"/*.rs | xargs -n1 basename | sed 's/\.rs$//'); do
  TIMEBOX="${MIRI_TIMEBOX:-900}" run "upstream-x86-avx2-integ-$t" "$WORK/zune-image/crates/zune-jpeg" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test --test "$t" --target $X86 -- $SKIP
done
# The same crate's tests without forcing AVX2: the AVX2 code does not run.
run upstream-x86-default    "$WORK/zune-image/crates/zune-jpeg" cargo +"$NIGHTLY" miri test --lib --target $X86 -- $SKIP
# The skipped test, alone: expected to fail under Miri, and recorded as such.
run upstream-x86-fnptr-test "$WORK/zune-image/crates/zune-jpeg" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test --lib --target $X86 -- set_options_refreshes_cached_dispatch
# This repository's seeds through the shipped crate.
run seeds-x86-avx2          "$ROOT" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test -p lumen-decoder-tools --test miri_jpeg --target $X86
# NEON on the host: expected to stop at a Miri limitation on aarch64 hosts.
run seeds-host-neon         "$ROOT" cargo +"$NIGHTLY" miri test -p lumen-decoder-tools --test miri_jpeg
echo "logs in $WORK"

# The record, written from the logs and not by hand.
python3 "$ROOT/scripts/decoder-miri-record.py" "$WORK" "$ROOT" "$COMMIT" "$NIGHTLY"
if [ -n "$COMPARE" ]; then
  cargo run -q --locked -p lumen-decoder-tools -- miri-check "$WORK/miri-record.json" "$COMPARE"
fi
