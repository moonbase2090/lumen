#!/bin/sh
# Exercise real grid wrapping, link activation and rendered underlines.
set -eu

cd "$(dirname "$0")/.."
proof_dir="${LUMEN_PROOF_DIR:-target/wrapped-link-proof}"
mkdir -p "$proof_dir"
LUMEN_PROOF_DIR="$(cd "$proof_dir" && pwd)"
export LUMEN_PROOF_DIR

cargo test --locked -p lumen-term --lib linkhit::tests
cargo test --locked -p lumen-term --test link_window -- --nocapture
