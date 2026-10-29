#!/bin/sh
set -eu

cargo test --locked -p lumen-sealed
cargo clippy --locked -p lumen-panes --all-targets --all-features -- -D warnings
cargo test --locked -p lumen-panes --all-features
