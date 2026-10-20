#!/bin/sh
# The development skill an agent gets is the same bytes in this repository,
# in the packaged contract crate, and in the release artifacts, whenever the
# skill is committed.
set -eu

fail() {
  echo "test-release-artifacts: $*" >&2
  exit 1
}

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
cmp "$skill" "$tmp/bloom-petal-contract-$version/$skill" ||
  fail "the packaged crate's $skill differs from the repository's"

# The release artifacts are complete, verify against their checksums, and name
# the commit they were built from and the contract the CLI reports.
scripts/assemble-release-artifacts.sh "$tmp/dist"
(cd "$tmp/dist" && shasum -a 256 -c SHA256SUMS >/dev/null)
inspect=$(cargo run --quiet -p bloom-petal-cli -- inspect)
contract=$(printf '%s\n' "$inspect" | awk '/^contract:/ { print $2 }')
digest=$(printf '%s\n' "$inspect" | awk '/^wit_digest:/ { print $2 }')
[ -n "$contract" ] && [ -n "$digest" ] || fail "petal inspect reported no contract or wit_digest"
grep -qx "contract: $contract" "$tmp/dist/contract.txt" ||
  fail "HEAD's wit/route/route.wit declares $(sed -n 's/^contract: //p' "$tmp/dist/contract.txt"), but petal inspect reports $contract"
listed=$(awk '{ print $2 }' "$tmp/dist/SHA256SUMS" | LC_ALL=C sort | tr '\n' ' ')
expected="SKILL.md contract.txt petal-wit-${contract#bloom:route@}.tar.gz "
[ "$listed" = "$expected" ] || fail "SHA256SUMS lists '$listed', expected '$expected'"
grep -qx "source_commit: $(git rev-parse HEAD)" "$tmp/dist/contract.txt" ||
  fail "contract.txt does not name HEAD as its source commit"
grep -qx "wit_digest: $digest" "$tmp/dist/contract.txt" ||
  fail "contract.txt does not carry wit_digest $digest"
# petal inspect hashes the working tree's WIT; the archive holds HEAD's.
git diff --quiet HEAD -- wit src/lib.rs ||
  echo "test-release-artifacts: wit or src/lib.rs has uncommitted changes; contract.txt's wit_digest describes the working tree, not the archived WIT" >&2

# The release artifact is HEAD's skill, byte for byte, whatever the working
# tree holds.
git show "HEAD:$skill" | cmp - "$tmp/dist/SKILL.md" ||
  fail "the release artifact's SKILL.md differs from HEAD's"

# The packaged crate carries the working tree's skill, so it matches the
# release artifact only when the skill is committed.
if git diff --quiet HEAD -- "$skill"; then
  cmp "$tmp/bloom-petal-contract-$version/$skill" "$tmp/dist/SKILL.md" ||
    fail "the packaged crate's $skill differs from the release artifact"
elif ! cmp -s "$tmp/bloom-petal-contract-$version/$skill" "$tmp/dist/SKILL.md"; then
  echo "test-release-artifacts: $skill has uncommitted changes; the packaged crate and the release artifact differ" >&2
fi
