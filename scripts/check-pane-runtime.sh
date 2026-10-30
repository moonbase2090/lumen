#!/bin/sh
set -eu

cargo test --locked -p lumen-sealed
cargo clippy --locked -p lumen-panes --all-targets --all-features -- -D warnings
cargo test --locked -p lumen-panes --all-features
cargo clippy --locked -p lumen-evidence --all-targets -- -D warnings
cargo test --locked -p lumen-evidence --test pane_process
