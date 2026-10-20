#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist at
# the repository root). A relative directory is taken from the caller's
# working directory. The directory is replaced, so it must be new, empty, or
# hold only a previous run's artifacts.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
case ${1:-} in
  '') out=$root/dist ;;
  /*) out=$1 ;;
  *) out=$PWD/$1 ;;
esac
cd "$root"

refuse() {
  echo "assemble-release-artifacts: refusing to replace $out, $*" >&2
  exit 1
}
if [ -e "$out" ] && [ -n "$(ls -A "$out")" ]; then
  [ -f "$out/SHA256SUMS" ] || refuse "which holds no previous artifacts"
  for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    case ${entry##*/} in
      SHA256SUMS | SKILL.md | contract.txt | petal-wit-*.tar.gz | .DS_Store) ;;
      *) refuse "which holds ${entry##*/}, not a release artifact" ;;
    esac
  done
fi

# Name the archive after the package its WIT declares, so a contract bump
# cannot ship under the previous version.
contract=$(git show HEAD:wit/route/route.wit | sed -n 's/^package \(bloom:route@[^;]*\);$/\1/p')
if [ -z "$contract" ]; then
  echo "assemble-release-artifacts: HEAD:wit/route/route.wit declares no bloom:route package" >&2
  exit 1
fi
wit=petal-wit-${contract#bloom:route@}
commit=$(git rev-parse HEAD)
inspect=$(cargo run --quiet -p bloom-petal-cli -- inspect)
digest=$(printf '%s\n' "$inspect" | awk '/^wit_digest:/ { print $2 }')
if [ -z "$digest" ]; then
  echo "assemble-release-artifacts: petal inspect reported no wit_digest" >&2
  exit 1
fi

# Replace the directory only once everything that builds has succeeded.
rm -rf "$out"
mkdir -p "$out"
git archive --format=tar.gz --prefix="$wit/" -o "$out/$wit.tar.gz" HEAD:wit/route
cat >"$out/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF
git show HEAD:skills/bloom-petal-development/SKILL.md >"$out/SKILL.md"

(cd "$out" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)
