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

rm -rf dist
mkdir -p dist
git archive --format=tar.gz --prefix=petal-wit-0.1.0/ \
  -o dist/petal-wit-0.1.0.tar.gz HEAD:wit/route

commit=$(git rev-parse HEAD)
digest=$(cargo run --quiet -p bloom-petal-cli -- inspect | awk '/^wit_digest:/ { print $2 }')
cat >dist/contract.txt <<EOF
source_commit: $commit
contract: bloom:route@0.1.0
wit_digest: $digest
EOF

(cd dist && shasum -a 256 contract.txt petal-wit-0.1.0.tar.gz >SHA256SUMS)
echo "release artifacts written to $root/dist"
