#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
wasm-tools component wit wit/route >/dev/null
for package in wit/route/deps/*; do
  wasm-tools component wit "$package" >/dev/null
done
scripts/generate-bindings.sh
git diff --exit-code -- crates/petal-sdk/src/route_file.rs

