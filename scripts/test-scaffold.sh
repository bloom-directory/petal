#!/bin/sh
# Take a scaffold from `petal new` through a new developer's first build:
# route tests, the architecture check, component build and capability check.
#
# The scaffold pins this version's release tag, and its scripts install the
# petal CLI from the commit that tag names. Neither is published while this
# version is a candidate, so a throwaway Git repository holds this checkout
# on master and under that tag, and Git config sends the SDK repository's URL
# there. The same repository tests a commit pin, which a Petal uses until the
# release is tagged.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/petal-scaffold.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
tmp=$(cd "$tmp" && pwd -P)

cargo build --quiet --locked --manifest-path "$root/Cargo.toml" -p bloom-petal-cli
petal="$root/target/debug/petal"
version=$("$petal" --version | sed 's/^petal //')

mkdir "$tmp/sdk"
(
  cd "$root"
  git ls-files -z --cached --others --exclude-standard |
    xargs -0 sh -c 'for f; do [ -e "$f" ] && printf "%s\0" "$f"; done' sh |
    tar -c --null -T - -f - | tar -x -f - -C "$tmp/sdk"
)
git -C "$tmp/sdk" init --quiet
git -C "$tmp/sdk" symbolic-ref HEAD refs/heads/master
git -C "$tmp/sdk" add -A
git -C "$tmp/sdk" -c user.name=test -c user.email=test@example.invalid \
  commit --quiet --no-verify -m "v$version candidate"
# An annotated tag, so petal.sh must read the commit it names, not the tag.
git -C "$tmp/sdk" -c user.name=test -c user.email=test@example.invalid \
  tag -a -m "v$version" "v$version"
git -C "$tmp/sdk" tag v9.9.8
GIT_CONFIG_GLOBAL="$tmp/gitconfig"
git config --file "$GIT_CONFIG_GLOBAL" \
  url."file://$tmp/sdk".insteadOf https://github.com/bloom-directory/petal
CARGO_NET_GIT_FETCH_WITH_CLI=true
PETAL_TOOLS_DIR="$tmp/tools"
export GIT_CONFIG_GLOBAL CARGO_NET_GIT_FETCH_WITH_CLI PETAL_TOOLS_DIR

"$petal" new demo "$tmp/demo" >/dev/null
cd "$tmp/demo"
for script in scripts/petal.sh scripts/build.sh scripts/check-route-architecture.sh; do
  test -x "$script" || { echo "test-scaffold: $script is not executable" >&2; exit 1; }
done
grep -q "tag = \"v$version\"" route/Cargo.toml || {
  echo "test-scaffold: route/Cargo.toml does not pin tag v$version" >&2
  exit 1
}

cargo test --quiet --manifest-path route/Cargo.toml
grep -q "source = \"git+https://github.com/bloom-directory/petal?tag=v$version#" route/Cargo.lock
bash scripts/check-route-architecture.sh
scripts/build.sh >/dev/null
scripts/petal.sh check >/dev/null
test "$("$PETAL_TOOLS_DIR"/*/bin/petal --version)" = "petal $version"
test -f 'petal/demo/account/[wallet]/[index]/address.json.wasm' || {
  echo "test-scaffold: the account route was not built:" >&2
  find petal -type f >&2
  exit 1
}

# GitHub serves any fork's commit at the SDK's URL, so petal.sh installs only
# the commit the tag names, and refuses anything else before installing.
real=$(git -C "$tmp/sdk" rev-parse HEAD)
other=0123456789abcdef0123456789abcdef01234567
sdk_url=https://github.com/bloom-directory/petal
cp route/Cargo.lock "$tmp/Cargo.lock"
refuses() {
  expected=$1 source=$2
  sed "s|^source = \"git+$sdk_url?tag=v$version#$real\"$|source = \"$source\"|" \
    "$tmp/Cargo.lock" >route/Cargo.lock
  if PETAL_TOOLS_DIR="$tmp/unverified" scripts/petal.sh --version 2>"$tmp/refusal"; then
    echo "test-scaffold: petal.sh accepted $source" >&2
    exit 1
  fi
  grep -qF "$expected" "$tmp/refusal" || {
    echo "test-scaffold: petal.sh refused $source without \"$expected\":" >&2
    cat "$tmp/refusal" >&2
    exit 1
  }
}
refuses "names $real for v$version" "git+$sdk_url?tag=v$version#$other"
refuses "names $real for v$version" "git+$sdk_url.git?tag=v$version#$other"
refuses "names $real for v9.9.8" "git+$sdk_url?tag=v9.9.8#$other"
refuses "names no commit for v9.9.9" "git+$sdk_url?tag=v9.9.9#$real"
refuses "a full commit" "git+$sdk_url?tag=v$version#../../../../tmp/x"
refuses "a full commit" "git+$sdk_url?tag=main#$real"
# A commit pin is taken only when the commit is on master or under a release
# tag. Stand-in commits for each case, all children of the released one:
sdk_git() {
  git -C "$tmp/sdk" -c user.name=test -c user.email=test@example.invalid "$@"
}
tree=$(sdk_git rev-parse "$real^{tree}")
# on master, under no tag: what a Petal pins until the release is tagged;
sdk_git commit --quiet --allow-empty -m "merged after the tag"
untagged=$(sdk_git rev-parse HEAD)
# under a release tag, not on master;
tagonly=$(sdk_git commit-tree -p "$real" -m "release branch" "$tree")
sdk_git tag v9.9.6 "$tagonly"
# on another branch, which the check never fetches, as with a fork's commit;
unmerged=$(sdk_git commit-tree -p "$real" -m "unmerged work" "$tree")
sdk_git branch --quiet unmerged "$unmerged"
# under a tag that is not a release, which the check does fetch;
prerelease=$(sdk_git commit-tree -p "$real" -m "not a release" "$tree")
sdk_git tag vnext "$prerelease"
# and an annotated tag's own id, which Git would read as the commit it names.
tagobject=$(sdk_git rev-parse "refs/tags/v$version")
test "$tagobject" != "$real"

refuses "not on master or a release tag" "git+$sdk_url?rev=$other#$other"
refuses "not on master or a release tag" "git+$sdk_url?rev=$unmerged#$unmerged"
refuses "not on master or a release tag" "git+$sdk_url?rev=$prerelease#$prerelease"
refuses "not on master or a release tag" "git+$sdk_url?rev=$tagobject#$tagobject"
refuses "a full commit" "git+$sdk_url?rev=${real%????????}#${real%????????}"
refuses "a full commit" "git+$sdk_url?tag=#$real"
refuses "pin a release tag or a commit" "git+$sdk_url?branch=master#$real"
refuses "pin a release tag or a commit" "git+https://github.com/someone/petal?rev=$real#$real"
# No refusal installed a CLI, recorded a check, or left its working repository.
left=$(find "$tmp/unverified" \( -name bin -o -name checked-commit -o -name 'sdk-history.*' \) 2>/dev/null)
if [ -n "$left" ]; then
  echo "test-scaffold: refused pins left these under $tmp/unverified:" >&2
  echo "$left" >&2
  exit 1
fi

# A CLI already in the cache does not excuse the check. Give each commit the
# CLI built above, as an earlier unchecked install would have.
seed() {
  mkdir -p "$1/$2"
  cp -R "$PETAL_TOOLS_DIR/$real/bin" "$1/$2/bin"
}
pin_commit() {
  sed "s|?tag=v$version#$real\"$|?rev=$1#$1\"|" "$tmp/Cargo.lock" >route/Cargo.lock
  grep -q "source = \"git+$sdk_url?rev=$1#$1\"" route/Cargo.lock
}
for commit in "$unmerged" "$prerelease"; do
  seed "$tmp/seeded" "$commit"
  pin_commit "$commit"
  if PETAL_TOOLS_DIR="$tmp/seeded" scripts/petal.sh --version >/dev/null 2>"$tmp/refusal"; then
    echo "test-scaffold: petal.sh ran a cached CLI for unchecked commit $commit" >&2
    exit 1
  fi
  grep -qF "not on master or a release tag" "$tmp/refusal"
  test ! -e "$tmp/seeded/$commit/checked-commit"
done

# Each accepted case is checked before its cached CLI runs, on its own
# ground: master alone, then a release tag alone. Once checked, a commit is
# not checked again, so the CLI runs offline.
for commit in "$untagged" "$tagonly" "$real"; do
  seed "$tmp/seeded" "$commit"
  pin_commit "$commit"
  test "$(PETAL_TOOLS_DIR="$tmp/seeded" scripts/petal.sh --version)" = "petal $version"
  test -f "$tmp/seeded/$commit/checked-commit"
  test "$(PETAL_TOOLS_DIR="$tmp/seeded" GIT_CONFIG_GLOBAL=/dev/null GIT_ALLOW_PROTOCOL=none \
    scripts/petal.sh --version)" = "petal $version"
done
# The CLI installed for the tag pin was checked for that tag only, so the same
# commit as a commit pin is checked too.
pin_commit "$real"
test ! -e "$PETAL_TOOLS_DIR/$real/checked-commit"
test "$(scripts/petal.sh --version)" = "petal $version"
test -f "$PETAL_TOOLS_DIR/$real/checked-commit"

# Git gives a hook GIT_DIR. A check run from one must not fetch into that
# repository, where it would move the Petal's own master and tags.
git init --quiet "$tmp/hooked-repo"
git -C "$tmp/hooked-repo" symbolic-ref HEAD refs/heads/work
git -C "$tmp/hooked-repo" -c user.name=test -c user.email=test@example.invalid \
  commit --quiet --allow-empty -m "a Petal's own history"
git -C "$tmp/hooked-repo" branch --quiet master
git -C "$tmp/hooked-repo" tag "v$version"
hooked_refs=$(git -C "$tmp/hooked-repo" show-ref)
seed "$tmp/hooked" "$untagged"
pin_commit "$untagged"
test "$(GIT_DIR="$tmp/hooked-repo/.git" PETAL_TOOLS_DIR="$tmp/hooked" \
  scripts/petal.sh --version)" = "petal $version"
test -f "$tmp/hooked/$untagged/checked-commit"
if [ "$(git -C "$tmp/hooked-repo" show-ref)" != "$hooked_refs" ]; then
  echo "test-scaffold: a commit check run with GIT_DIR set changed that repository's refs:" >&2
  git -C "$tmp/hooked-repo" show-ref >&2
  exit 1
fi
# No check left its working repository behind.
if find "$tmp/seeded" "$tmp/hooked" "$PETAL_TOOLS_DIR" -maxdepth 1 -name 'sdk-history.*' | grep -q .; then
  echo "test-scaffold: a commit check left its working repository in the cache" >&2
  exit 1
fi
cp "$tmp/Cargo.lock" route/Cargo.lock

# The CLI installed above was checked for v$version only. Another tag that
# names a different commit is refused before that CLI runs, and so is a CLI
# that an earlier petal.sh installed without checking.
git -C "$tmp/sdk" -c user.name=test -c user.email=test@example.invalid \
  commit --quiet --allow-empty -m "v9.9.7 candidate"
git -C "$tmp/sdk" tag v9.9.7
cached=$PETAL_TOOLS_DIR/$real
test -f "$cached/checked-tags/v$version"
sed "s|?tag=v$version#|?tag=v9.9.7#|" "$tmp/Cargo.lock" >route/Cargo.lock
if scripts/petal.sh --version 2>"$tmp/refusal"; then
  echo "test-scaffold: petal.sh ran the CLI cached for v$version under v9.9.7" >&2
  exit 1
fi
grep -qF "names $(git -C "$tmp/sdk" rev-parse v9.9.7) for v9.9.7" "$tmp/refusal"
test ! -e "$cached/checked-tags/v9.9.7"
cp "$tmp/Cargo.lock" route/Cargo.lock
mv "$cached/checked-tags" "$tmp/checked-tags"
test "$(scripts/petal.sh --version)" = "petal $version"
test -f "$cached/checked-tags/v$version"
# Once checked, the tag is not checked again, so the CLI runs offline.
test "$(GIT_CONFIG_GLOBAL=/dev/null GIT_ALLOW_PROTOCOL=none scripts/petal.sh --version)" = "petal $version"

echo "test-scaffold: petal new $version builds, tests and checks"
