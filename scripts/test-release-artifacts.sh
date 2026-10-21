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
trap 'printf "Release test evidence retained at %s\n" "$tmp"' EXIT
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
# the commit they were built from and the contract the CLI reports. The test
# also runs on uncommitted work, so it lets the assembler use a dirty tree and
# warns about it below; an uncommitted contract bump still fails until it is
# committed.
scripts/assemble-release-artifacts.sh --allow-dirty "$tmp/dist"
(cd "$tmp/dist" && shasum -a 256 -c SHA256SUMS >/dev/null)
sleep 2
scripts/assemble-release-artifacts.sh --allow-dirty "$tmp/repeated"
cmp "$tmp/dist/SHA256SUMS" "$tmp/repeated/SHA256SUMS" ||
  fail "release artifacts changed when only build time changed"
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
[ -z "$(git status --porcelain --untracked-files=all -- wit src)" ] ||
  echo "test-release-artifacts: wit/ or src/ has uncommitted changes; contract.txt's wit_digest describes the working tree, not the archived WIT" >&2

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

# Without --allow-dirty, the assembler refuses a tree whose WIT inputs differ
# from HEAD, before it touches a previous run's artifacts. This runs it in a
# scratch repository, so the refusal comes before it needs cargo.
repo=$tmp/guard
mkdir -p "$repo/scripts" "$repo/wit/route" "$repo/src" "$repo/prev"
cp scripts/assemble-release-artifacts.sh "$repo/scripts/"
printf 'package bloom:route@0.1.0;\n' >"$repo/wit/route/route.wit"
: >"$repo/src/lib.rs"
printf 'previous\n' >"$repo/prev/contract.txt"
: >"$repo/prev/SHA256SUMS"
git -C "$repo" init -q
git -C "$repo" add scripts wit src
git -C "$repo" -c user.name=test -c user.email=test@example.invalid \
  -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -qm init
for change in modified staged untracked; do
  case $change in
    modified) printf '// change\n' >>"$repo/wit/route/route.wit" ;;
    staged) printf '// change\n' >>"$repo/src/lib.rs" && git -C "$repo" add src/lib.rs ;;
    untracked) mkdir -p "$repo/wit/route/deps/new" && printf 'package a:b;\n' >"$repo/wit/route/deps/new/new.wit" ;;
  esac
  status=0
  "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
  [ "$status" = 1 ] && grep -q 'differs from HEAD' "$tmp/guard.err" ||
    fail "the assembler did not refuse a $change WIT input: exit $status, $(cat "$tmp/guard.err")"
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts despite a $change WIT input"
  git -C "$repo" restore --source=HEAD --staged --worktree -- wit/route/route.wit src/lib.rs
  if [ -f "$repo/wit/route/deps/new/new.wit" ]; then
    rm -f "$repo/wit/route/deps/new/new.wit"
    rmdir "$repo/wit/route/deps/new" "$repo/wit/route/deps"
  fi
done

mkdir -p "$tmp/directory-guard/SKILL.md"
printf 'previous\n' > "$tmp/directory-guard/contract.txt"
: > "$tmp/directory-guard/SHA256SUMS"
status=0
scripts/assemble-release-artifacts.sh "$tmp/directory-guard" 2>"$tmp/directory-guard.err" || status=$?
[ "$status" = 1 ] && grep -q 'holds directory SKILL.md' "$tmp/directory-guard.err" ||
  fail "artifact-named directory was not refused before replacement"
[ "$(cat "$tmp/directory-guard/contract.txt")" = previous ] ||
  fail "an invalid output directory lost previous artifacts"
