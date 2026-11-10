#!/bin/sh
# Take a scaffold from `petal new` through a new developer's first build:
# route tests, the architecture check, component build and capability check.
#
# The scaffold pins this version's release tag, and its scripts install the
# petal CLI from the commit that tag names. Neither is published while this
# version is a candidate, so a throwaway Git repository holds this checkout
# under that tag, and Git config sends the SDK repository's URL there.
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
refuses "pin a release tag" "git+$sdk_url?rev=$real#$real"
refuses "a full commit" "git+$sdk_url?tag=v$version#../../../../tmp/x"
refuses "a full commit" "git+$sdk_url?tag=main#$real"
cp "$tmp/Cargo.lock" route/Cargo.lock
test ! -e "$tmp/unverified"

echo "test-scaffold: petal new $version builds, tests and checks"
