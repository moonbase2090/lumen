#!/bin/bash -eu
# Builds the decoder fuzz targets for ClusterFuzzLite. The nightly is pinned
# because the toolchain is part of the build identity (see scripts/decoder-fuzz-vm.sh).
NIGHTLY="${NIGHTLY:-nightly-2026-10-06}"
rustup toolchain install "$NIGHTLY" --profile minimal
cd "$SRC/lumen/crates/lumen-decoder"
cargo +"$NIGHTLY" fuzz build -O --sanitizer address
FUZZ_TARGET_OUTPUT_DIR=fuzz/target/x86_64-unknown-linux-gnu/release
for f in fuzz/fuzz_targets/*.rs; do
  name=$(basename "$f" .rs)
  cp "$FUZZ_TARGET_OUTPUT_DIR/$name" "$OUT/"
done
# Seed corpus: the committed regression seeds, one zip per format.
for fmt in jpeg png webp; do
  [ -d "fuzz/seeds-regression/$fmt" ] && zip -qj "$OUT/decode_${fmt}_seed_corpus.zip" fuzz/seeds-regression/"$fmt"/* || true
done
