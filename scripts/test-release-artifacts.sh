#!/bin/sh
# The development skill an agent gets is the same bytes in this repository,
# in the packaged contract crate, and in the release artifacts.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
skill=skills/bloom-petal-development/SKILL.md

# The crate ships the skill and builds from its own tarball, so the
# include_str! behind PETAL_DEVELOPMENT_SKILL resolves for its consumers.
cargo package -p bloom-petal-contract --allow-dirty --quiet
version=$(cargo pkgid -p bloom-petal-contract | sed 's/.*[#@]//')
target=$(cargo metadata --format-version 1 --no-deps |
  sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
tar -xzf "$target/package/bloom-petal-contract-$version.crate" -C "$tmp"
cmp "$skill" "$tmp/bloom-petal-contract-$version/$skill"

# The release artifacts are complete, verify against their checksums, and name
# the commit they were built from.
scripts/assemble-release-artifacts.sh "$tmp/dist"
(cd "$tmp/dist" && shasum -a 256 -c SHA256SUMS >/dev/null)
listed=$(awk '{ print $2 }' "$tmp/dist/SHA256SUMS" | LC_ALL=C sort | tr '\n' ' ')
test "$listed" = "SKILL.md contract.txt petal-wit-0.1.0.tar.gz "
grep -qx "source_commit: $(git rev-parse HEAD)" "$tmp/dist/contract.txt"

# Release artifacts come from HEAD, so they match the crate only when the
# skill is committed.
if git diff --quiet HEAD -- "$skill"; then
  cmp "$skill" "$tmp/dist/SKILL.md"
else
  echo "test-release-artifacts: $skill has uncommitted changes; checked the packaged crate only" >&2
fi
