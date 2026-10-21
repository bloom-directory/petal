#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist at
# the repository root). A relative directory is taken from the caller's
# working directory. The directory is replaced, so it must be new, empty, or
# hold only a previous run's artifacts, and not a symbolic link. The artifacts
# are written beside it and moved in at the end, so a failure leaves a previous
# run's artifacts as they were.
#
# contract.txt's wit_digest comes from the working tree's contract crate and
# the archive from HEAD, so the script refuses to run while wit/ or src/
# differs from HEAD, untracked files included, or holds files git is told to
# ignore changes to. --allow-dirty skips that check, as it does for cargo
# package; only tests use it.
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
# Drop trailing slashes, so a symbolic link is seen as one and the final move
# names the directory itself.
while [ "$out" != / ] && [ "${out%/}" != "$out" ]; do
  out=${out%/}
done
cd "$root"

if [ -z "$allow_dirty" ]; then
  dirty=$(git status --porcelain --untracked-files=all -- wit src)
  if [ -n "$dirty" ]; then
    echo "assemble-release-artifacts: wit/ or src/ differs from HEAD, so the wit_digest would describe the working tree, not the archived WIT; commit or stash:" >&2
    printf '%s\n' "$dirty" >&2
    exit 1
  fi
  # git status cannot see a change behind assume-unchanged (lowercase tag) or
  # skip-worktree (S).
  hidden=$(git ls-files -v -- wit src | grep '^[a-zS]' || true)
  if [ -n "$hidden" ]; then
    echo "assemble-release-artifacts: wit/ or src/ has files git is told to ignore changes to, so the guard cannot check them; clear assume-unchanged or skip-worktree:" >&2
    printf '%s\n' "$hidden" >&2
    exit 1
  fi
fi

refuse() {
  echo "assemble-release-artifacts: refusing to replace $out, $*" >&2
  exit 1
}
if [ -L "$out" ]; then
  refuse "which is a symbolic link"
fi
if [ -e "$out" ] && [ ! -d "$out" ]; then
  refuse "which is not a directory"
fi
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

# Write the artifacts beside the directory and move them in only once all of
# them exist, so a failure before then leaves a previous run's artifacts as
# they were. Only validated artifact files are ever deleted: a directory with
# an artifact's name is never recursive-cleanup authority.
parent=$(dirname -- "$out")
mkdir -p "$parent"
stage=$(mktemp -d "$parent/.assemble-release-artifacts.XXXXXX")
cleanup() {
  for entry in "$stage"/*; do
    if [ -f "$entry" ]; then
      rm -f -- "$entry"
    fi
  done
  rmdir "$stage" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
# A subtree archive otherwise uses wall-clock time. Bind it to its source commit.
archive_time=$(git show -s --format=%cI HEAD)
git archive --format=tar.gz --mtime="$archive_time" --prefix="$wit/" -o "$stage/$wit.tar.gz" HEAD:wit/route
cat >"$stage/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF
git show HEAD:skills/bloom-petal-development/SKILL.md >"$stage/SKILL.md"
(cd "$stage" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)

if [ -d "$out" ]; then
  for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    rm -f -- "$entry"
  done
fi
mkdir -p "$out"
for name in SHA256SUMS contract.txt SKILL.md "$wit.tar.gz"; do
  mv -- "$stage/$name" "$out/$name"
done
