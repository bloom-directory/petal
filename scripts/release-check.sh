#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

scripts/check.sh
cargo package -p bloom-petal-contract
cargo package -p bloom-petal-sdk
if cargo search bloom-petal-contract --limit 1 | grep -q '^bloom-petal-contract = "0.1.0"'; then
  cargo package -p bloom-petal-builder
fi
if cargo search bloom-petal-builder --limit 1 | grep -q '^bloom-petal-builder = "0.1.0"'; then
  cargo package -p bloom-petal-cli
fi

scripts/assemble-release-artifacts.sh dist
echo "release artifacts written to $root/dist"
