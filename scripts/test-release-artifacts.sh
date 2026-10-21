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
package_id=$(cargo pkgid -p bloom-petal-contract)
version=${package_id##*[#@]}
target=$(cargo metadata --format-version 1 --no-deps |
  sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
tar -xzf "$target/package/bloom-petal-contract-$version.crate" -C "$tmp"
cmp "$skill" "$tmp/bloom-petal-contract-$version/$skill" ||
  fail "the packaged crate's $skill differs from the repository's"

# The release artifacts are complete, verify against their checksums, and name
# the commit they were built from and the contract the CLI reports. On a clean
# tree, as in CI, the assembler runs exactly as a release runs it. The test
# also runs on uncommitted work, so there it lets the assembler use a dirty
# tree and warns about it below; an uncommitted contract bump still fails until
# it is committed.
dirty=$(git status --porcelain --untracked-files=all -- wit src
  git ls-files -v -- wit src | grep '^[a-zS]' || true)
assemble() {
  if [ -z "$dirty" ]; then
    scripts/assemble-release-artifacts.sh "$@"
  else
    scripts/assemble-release-artifacts.sh --allow-dirty "$@"
  fi
}
assemble "$tmp/dist"
(cd "$tmp/dist" && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "the release artifacts do not verify against SHA256SUMS"
sleep 2
assemble "$tmp/repeated"
cmp "$tmp/dist/SHA256SUMS" "$tmp/repeated/SHA256SUMS" ||
  fail "release artifacts changed when only build time changed"

# The output must be named directly. A final . or .. is refused, and the
# previous artifacts stay.
for name in "$tmp/dist/." "$tmp/dist/sub/.."; do
  status=0
  assemble "$name" 2>"$tmp/dot.err" || status=$?
  [ "$status" = 2 ] || fail "the assembler accepted $name as its output: exit $status"
done
(cd "$tmp/dist" && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "a refused output lost its previous artifacts"

# Assembling over a previous run replaces exactly its artifact files, stale
# ones and AppleDouble companions included.
mkdir "$tmp/stale"
for name in SHA256SUMS SKILL.md contract.txt petal-wit-0.0.0.tar.gz .DS_Store ._SKILL.md; do
  printf 'stale\n' >"$tmp/stale/$name"
done
assemble "$tmp/stale"
listed=$(ls -A "$tmp/stale" | LC_ALL=C sort | tr '\n' ' ')
expected=$(ls -A "$tmp/dist" | LC_ALL=C sort | tr '\n' ' ')
[ "$listed" = "$expected" ] ||
  fail "assembling over a previous run left '$listed', expected '$expected'"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "assembling over a previous run gave different artifacts"

# A failure after the artifacts are staged keeps the previous ones and leaves
# no staging directory. The stub only fails the assembler's own checksum step.
mkdir "$tmp/stub"
printf '#!/bin/sh\nexit 1\n' >"$tmp/stub/shasum"
chmod +x "$tmp/stub/shasum"
status=0
(PATH=$tmp/stub:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/stub.err" || status=$?
[ "$status" != 0 ] || fail "the assembler succeeded although its checksum step failed"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a failure after staging lost the previous artifacts"
for entry in "$tmp"/.assemble-release-artifacts.*; do
  [ ! -e "$entry" ] || fail "a failure after staging left $entry"
done
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
[ -z "$dirty" ] ||
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
# from HEAD, or that hides changes from git, before it touches a previous run's
# artifacts; it also refuses a symbolic link as its output, with or without a
# trailing slash. This runs it in a scratch repository, so each refusal comes
# before it needs cargo. The subshell drops the caller's git environment,
# config and attributes: run from a hook or `git rebase -x`, GIT_DIR and
# GIT_INDEX_FILE would otherwise point these commands at the caller's
# repository.
(
  unset $(git rev-parse --local-env-vars)
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_ATTR_NOSYSTEM=1
  repo=$tmp/guard
  mkdir -p "$repo/scripts" "$repo/wit/route" "$repo/src" "$repo/prev"
  cp scripts/assemble-release-artifacts.sh "$repo/scripts/"
  printf 'package bloom:route@0.0.0;\n' >"$repo/wit/route/route.wit"
  : >"$repo/src/lib.rs"
  printf 'previous\n' >"$repo/prev/contract.txt"
  : >"$repo/prev/SHA256SUMS"
  git -C "$repo" init -q --template=
  git -C "$repo" config core.excludesFile /dev/null
  git -C "$repo" config core.attributesFile /dev/null
  git -C "$repo" config user.name test
  git -C "$repo" config user.email test@example.invalid
  git -C "$repo" config commit.gpgsign false
  git -C "$repo" add scripts wit src
  git -C "$repo" commit -qm init
  for change in modified staged untracked assume-unchanged skip-worktree; do
    case $change in
      modified) printf '// change\n' >>"$repo/wit/route/route.wit" ;;
      staged) printf '// change\n' >>"$repo/src/lib.rs" && git -C "$repo" add src/lib.rs ;;
      untracked) mkdir -p "$repo/wit/route/deps/new" && printf 'package a:b;\n' >"$repo/wit/route/deps/new/new.wit" ;;
      *) git -C "$repo" update-index "--$change" wit/route/route.wit && printf '// change\n' >>"$repo/wit/route/route.wit" ;;
    esac
    status=0
    "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
    [ "$status" = 1 ] && grep -q '^assemble-release-artifacts: wit/ or src/' "$tmp/guard.err" ||
      fail "the assembler did not refuse a $change WIT input: exit $status, $(cat "$tmp/guard.err")"
    [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
      fail "the assembler replaced the previous artifacts despite a $change WIT input"
    git -C "$repo" update-index --no-assume-unchanged wit/route/route.wit
    git -C "$repo" update-index --no-skip-worktree wit/route/route.wit
    git -C "$repo" restore --source=HEAD --staged --worktree -- wit/route/route.wit src/lib.rs
    if [ -f "$repo/wit/route/deps/new/new.wit" ]; then
      rm -f "$repo/wit/route/deps/new/new.wit"
      rmdir "$repo/wit/route/deps/new" "$repo/wit/route/deps"
    fi
  done
  ln -s prev "$repo/link"
  for name in link link/; do
    status=0
    "$repo/scripts/assemble-release-artifacts.sh" "$repo/$name" 2>"$tmp/guard.err" || status=$?
    [ "$status" = 1 ] && grep -q 'symbolic link' "$tmp/guard.err" ||
      fail "the assembler did not refuse the symbolic link $name as its output: exit $status, $(cat "$tmp/guard.err")"
    [ -L "$repo/link" ] && [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
      fail "the assembler replaced a symbolic link or its target"
  done
)

# An artifact-named directory in the output is refused, not deleted.
mkdir -p "$tmp/directory-guard/SKILL.md"
printf 'previous\n' > "$tmp/directory-guard/contract.txt"
: > "$tmp/directory-guard/SHA256SUMS"
status=0
assemble "$tmp/directory-guard" 2>"$tmp/directory-guard.err" || status=$?
[ "$status" = 1 ] && grep -q 'holds directory SKILL.md' "$tmp/directory-guard.err" ||
  fail "artifact-named directory was not refused before replacement"
[ "$(cat "$tmp/directory-guard/contract.txt")" = previous ] ||
  fail "an invalid output directory lost previous artifacts"
