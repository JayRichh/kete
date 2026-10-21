#!/usr/bin/env bash
# The Linux half of the release gate: formatting and clippy over the whole
# workspace, cfg(unix) code included (Windows never compiles it). Run in a
# Linux container from the repo root, as the release plan shows.
set -euo pipefail
# rust:latest ships without these; a no-op where they are installed.
rustup component add rustfmt clippy
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
