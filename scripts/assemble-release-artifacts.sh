#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist at
# the repository root). A relative directory is taken from the caller's
# working directory. The directory is replaced, so it must be new, empty, or
# hold only a previous run's artifacts.
#
# contract.txt's wit_digest comes from the working tree's contract crate and
# the archive from HEAD, so the script refuses to run while wit/ or src/
# differs from HEAD, untracked files included. --allow-dirty skips that check,
# as it does for cargo package; only tests use it.
set -eu

usage() {
  echo "usage: assemble-release-artifacts.sh [--allow-dirty] [directory]" >&2
  exit 2
}
allow_dirty=
if [ "${1:-}" = --allow-dirty ]; then
  allow_dirty=1
  shift
fi
[ $# -le 1 ] || usage
case ${1:-} in
  -?*) usage ;;
esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
case ${1:-} in
  '') out=$root/dist ;;
  /*) out=$1 ;;
  *) out=$PWD/$1 ;;
esac
cd "$root"

if [ -z "$allow_dirty" ]; then
  dirty=$(git status --porcelain --untracked-files=all -- wit src)
  if [ -n "$dirty" ]; then
    echo "assemble-release-artifacts: wit/ or src/ differs from HEAD, so the wit_digest would describe the working tree, not the archived WIT; commit or stash:" >&2
    printf '%s\n' "$dirty" >&2
    exit 1
  fi
fi

refuse() {
  echo "assemble-release-artifacts: refusing to replace $out, $*" >&2
  exit 1
}
if [ -e "$out" ] && [ -n "$(ls -A "$out")" ]; then
  [ -f "$out/SHA256SUMS" ] || refuse "which holds no previous artifacts"
  for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    [ ! -d "$entry" ] || refuse "which holds directory ${entry##*/}"
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

# Replace only validated artifact files once everything that builds has succeeded.
# A directory with an artifact's name is never recursive-cleanup authority.
if [ -d "$out" ]; then
  for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    rm -f -- "$entry"
  done
fi
mkdir -p "$out"
# A subtree archive otherwise uses wall-clock time. Bind it to its source commit.
archive_time=$(git show -s --format=%cI HEAD)
git archive --format=tar.gz --mtime="$archive_time" --prefix="$wit/" -o "$out/$wit.tar.gz" HEAD:wit/route
cat >"$out/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF
git show HEAD:skills/bloom-petal-development/SKILL.md >"$out/SKILL.md"

(cd "$out" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)
