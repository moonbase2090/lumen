#!/usr/bin/env bash
# Runs Miri on zune-jpeg's unsafe code: its own tests at the commit that
# published 0.5.15, and the decoder's JPEG seeds. Needs a nightly with Miri and
# rust-src. The x86_64 target runs on any host (Miri interprets it); Miri
# reports no AVX2 unless the feature is compiled in, so it is forced on, or the
# AVX2 code would not run at all.
#
#   scripts/decoder-miri.sh WORKDIR     # WORKDIR is created; takes about 10 minutes
set -euo pipefail
NIGHTLY="${NIGHTLY:-nightly}"
WORK="${1:?usage: decoder-miri.sh WORKDIR}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UPSTREAM=https://github.com/etemesi254/zune-image
COMMIT=d22e2aa3fb4259943fe0dbe1b5fef47c7e72aab5   # crates/zune-jpeg version = "0.5.15"
X86=x86_64-unknown-linux-gnu
AVX2="-C target-feature=+avx2,+sse4.1"
export MIRIFLAGS="-Zmiri-disable-isolation"

mkdir -p "$WORK"; WORK="$(cd "$WORK" && pwd)"
[ -d "$WORK/zune-image" ] || git clone -q --filter=blob:none "$UPSTREAM" "$WORK/zune-image"
git -C "$WORK/zune-image" checkout -q "$COMMIT"

run() { # name, dir, args...
  name="$1"; dir="$2"; shift 2
  if (cd "$dir" && "$@" > "$WORK/$name.log" 2>&1); then st=ok; else st=FAILED; fi
  printf '%-34s %s  (%s)\n' "$name" "$st" "$(grep -E '^test result' "$WORK/$name.log" | tr '\n' ' ' | cut -c1-110)"
}

# zune-jpeg's own unit and integration tests, AVX2 forced on.
run upstream-x86-avx2-unit  "$WORK/zune-image/crates/zune-jpeg" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test --lib   --target $X86
run upstream-x86-avx2-integ "$WORK/zune-image/crates/zune-jpeg" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test --tests --target $X86
# The same crate's tests without forcing AVX2: the AVX2 code does not run.
run upstream-x86-default    "$WORK/zune-image/crates/zune-jpeg" cargo +"$NIGHTLY" miri test --lib --target $X86
# This repository's seeds through the shipped crate.
run seeds-x86-avx2          "$ROOT" env RUSTFLAGS="$AVX2" cargo +"$NIGHTLY" miri test -p lumen-decoder-tools --test miri_jpeg --target $X86
# NEON on the host: expected to stop at a Miri limitation on aarch64 hosts.
run seeds-host-neon         "$ROOT" cargo +"$NIGHTLY" miri test -p lumen-decoder-tools --test miri_jpeg
echo "logs in $WORK"
