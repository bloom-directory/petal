#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
out=${1:-dist}

rm -rf "$out"
mkdir -p "$out"
git archive --format=tar.gz --prefix=petal-wit-0.1.0/ \
  -o "$out/petal-wit-0.1.0.tar.gz" HEAD:wit/route

commit=$(git rev-parse HEAD)
digest=$(cargo run --quiet -p bloom-petal-cli -- inspect | awk '/^wit_digest:/ { print $2 }')
cat >"$out/contract.txt" <<EOF
source_commit: $commit
contract: bloom:route@0.1.0
wit_digest: $digest
EOF

git show HEAD:skills/bloom-petal-development/SKILL.md >"$out/SKILL.md"

(cd "$out" && shasum -a 256 contract.txt petal-wit-0.1.0.tar.gz SKILL.md >SHA256SUMS)
