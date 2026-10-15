#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
out=${1:-dist}

rm -rf "$out"
mkdir -p "$out"

# Name the archive after the package its WIT declares, so a contract bump
# cannot ship under the previous version.
contract=$(git show HEAD:wit/route/route.wit | sed -n 's/^package \(bloom:route@[^;]*\);$/\1/p')
if [ -z "$contract" ]; then
  echo "assemble-release-artifacts: HEAD:wit/route/route.wit declares no bloom:route package" >&2
  exit 1
fi
wit=petal-wit-${contract#bloom:route@}
git archive --format=tar.gz --prefix="$wit/" -o "$out/$wit.tar.gz" HEAD:wit/route

commit=$(git rev-parse HEAD)
digest=$(cargo run --quiet -p bloom-petal-cli -- inspect | awk '/^wit_digest:/ { print $2 }')
cat >"$out/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF

git show HEAD:skills/bloom-petal-development/SKILL.md >"$out/SKILL.md"

(cd "$out" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)
