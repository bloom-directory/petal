#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

scripts/check.sh
cargo package -p bloom-petal-contract
cargo package -p bloom-petal-sdk
# The builder and CLI package only once the version they depend on is on
# crates.io.
contract_id=$(cargo pkgid -p bloom-petal-contract)
builder_id=$(cargo pkgid -p bloom-petal-builder)
contract_version=${contract_id##*[#@]}
builder_version=${builder_id##*[#@]}
if cargo search bloom-petal-contract --limit 1 | grep -q "^bloom-petal-contract = \"$contract_version\""; then
  cargo package -p bloom-petal-builder
fi
if cargo search bloom-petal-builder --limit 1 | grep -q "^bloom-petal-builder = \"$builder_version\""; then
  cargo package -p bloom-petal-cli
fi

scripts/assemble-release-artifacts.sh dist
echo "release artifacts written to $root/dist"
