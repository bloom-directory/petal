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
cargo run --quiet -p bloom-petal-cli -- build --config fixtures/petal-build.toml
cargo run --quiet -p bloom-petal-cli -- check --config fixtures/petal-build.toml
test "$(wasm-tools component wit fixtures/components/static.txt.wasm | grep -c 'import bloom:sign/signing@0.1.0' || true)" = 0
test "$(wasm-tools component wit fixtures/components/sign.wasm | grep -c 'import bloom:sign/signing@0.1.0' || true)" = 1
