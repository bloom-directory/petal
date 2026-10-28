#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist at
# the repository root). A relative directory is taken from the caller's
# working directory. Its parent directory must already exist, and it must name
# the directory itself: it may not end in . or ..
#
# The directory is replaced, so it must be new, empty, or hold only a previous
# run's artifacts; it may not be a symbolic link, and this user must be able to
# read and write it. Before the build, the script enters the directory and
# does all its writing and deleting from inside it, so renaming or replacing
# the path, or any directory above it, cannot redirect it elsewhere; a new
# output is therefore created even if the build then fails. The new artifacts
# are staged in a hidden subdirectory and moved into place only once all of
# them exist: a failure before then leaves a previous run's artifacts as they
# were, and a failure while they are replaced names the directory that holds
# the new ones, which must be deleted before a rerun replaces them.
#
# The WIT archive takes its timestamps from HEAD's commit time, which needs git
# 2.40 or later.
#
# contract.txt's wit_digest comes from `petal inspect`, built from the working
# tree, while the archive comes from HEAD. So the script refuses to run while
# any file git tracks, or would list as untracked, differs from HEAD, or is
# hidden from git with assume-unchanged or skip-worktree; the output directory
# itself is exempt. Files git ignores are not checked, and Cargo configuration
# outside the repository and the build environment are the caller's to keep
# clean: this guard catches mistakes, it is not a security boundary. Every read
# names the commit HEAD pointed at when the script started, and the checks run
# again after the build, so a HEAD that moves meanwhile is refused. It also
# refuses a repository info/attributes file, which git archive always applies.
# --allow-dirty skips these checks, as it does for cargo package; only tests
# use it.
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
root=$(CDPATH= cd -P -- "$(dirname -- "$0")/.." && pwd -P)
case ${1:-} in
  '') out=$root/dist ;;
  /*) out=$1 ;;
  *) out=$PWD/$1 ;;
esac
# Drop trailing slashes, so a symbolic link is seen as one and the output is
# named by its last component.
while [ "$out" != / ] && [ "${out%/}" != "$out" ]; do
  out=${out%/}
done
# A final . or .. would name a different directory than the one checked.
case $out in
  */. | */..)
    echo "assemble-release-artifacts: name the output directory itself, not $out" >&2
    exit 2
    ;;
esac
# The parent must already exist, so it can be resolved now and compared with
# the directory the script later enters: a parent created or swapped in the
# meantime is refused. Resolving it also means a .. inside the path cannot
# make mkdir -p create a directory inside the output.
dir=${out%/*}
dir=${dir:-/}
if [ ! -d "$dir" ]; then
  echo "assemble-release-artifacts: $dir, the parent of $out, does not exist; create it first" >&2
  exit 2
fi
# The trailing . keeps a parent whose name ends in a newline intact.
resolved=$(CDPATH= cd -P -- "$dir" && pwd -P && echo .) || {
  echo "assemble-release-artifacts: cannot enter $dir, the parent of $out" >&2
  exit 1
}
resolved=${resolved%?.}
resolved_parent=${resolved%/}
out=$resolved_parent/${out##*/}
cd "$root"
# A replace ref would make git archive and git show read objects other than
# HEAD's, unseen by git status, so ignore replace refs throughout.
GIT_NO_REPLACE_OBJECTS=1
# The guard's output exclusion uses pathspec magic, which a caller's
# GIT_LITERAL_PATHSPECS would switch off.
GIT_LITERAL_PATHSPECS=0
export GIT_NO_REPLACE_OBJECTS GIT_LITERAL_PATHSPECS
# Every later read names this commit, so a HEAD that moves, or a repository
# path that is swapped, while the CLI builds cannot change what is archived.
commit=$(git rev-parse --verify 'HEAD^{commit}')

# guard runs before the build and again after it, so a change made while the
# CLI builds is refused too.
guard() {
  [ -z "$allow_dirty" ] || return 0
  if [ "$(git rev-parse --verify 'HEAD^{commit}')" != "$commit" ]; then
    echo "assemble-release-artifacts: HEAD moved from $commit while the artifacts were built; rerun" >&2
    exit 1
  fi
  # The output may sit inside the repository, for example dist/; leave it out.
  set -- .
  case $out in
    "$root"/?*) set -- . ":(exclude,literal)${out#"$root"/}" ;;
  esac
  dirty=$(git status --porcelain --untracked-files=all -- "$@")
  if [ -n "$dirty" ]; then
    echo "assemble-release-artifacts: the working tree differs from HEAD, so petal inspect, built from it, could report a wit_digest that does not describe HEAD's WIT; commit, stash or remove:" >&2
    printf '%s\n' "$dirty" >&2
    exit 1
  fi
  # git status cannot see a change behind assume-unchanged (lowercase tag) or
  # skip-worktree (S).
  hidden=$(git ls-files -v | grep '^[a-zS]' || true)
  if [ -n "$hidden" ]; then
    echo "assemble-release-artifacts: the repository has files git is told to ignore changes to, so the guard cannot check them; clear assume-unchanged or skip-worktree (for a sparse checkout, run git sparse-checkout disable):" >&2
    printf '%s\n' "$hidden" >&2
    exit 1
  fi
  # git archive always applies the repository's own info/attributes, and no
  # option turns that off, so it could drop or rewrite archived WIT files.
  info_attributes=$(git rev-parse --git-path info/attributes)
  if [ -s "$info_attributes" ]; then
    echo "assemble-release-artifacts: $info_attributes would change the WIT archive; move it aside" >&2
    exit 1
  fi
}
guard

refuse() {
  echo "assemble-release-artifacts: refusing to replace $out, $*" >&2
  exit 1
}
# check_out <path> [<staging name>]: the output, reached through <path>, must
# be a real directory holding nothing, or only a previous run's artifacts. This
# runs on the path first, then on . once the script is inside the output and
# before the build, and again on . just before the previous artifacts are
# replaced.
check_out() {
  if [ -L "$1" ]; then
    refuse "which is a symbolic link"
  fi
  if [ -e "$1" ] && [ ! -d "$1" ]; then
    refuse "which is not a directory"
  fi
  [ -e "$1" ] || return 0
  if [ ! -r "$1" ] || [ ! -w "$1" ] || [ ! -x "$1" ]; then
    refuse "which this user cannot read and write"
  fi
  ls -A -- "$1" >/dev/null || refuse "which cannot be listed"
  found=
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    [ "${entry##*/}" != "${2:-}" ] || continue
    found=1
  done
  [ -n "$found" ] || return 0
  for entry in "$1"/.assemble-release-artifacts.*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    [ "${entry##*/}" != "${2:-}" ] || continue
    refuse "which holds ${entry##*/}, the staging directory of an interrupted or concurrent run; once no run is using it, delete it and rerun"
  done
  [ -f "$1/SHA256SUMS" ] || refuse "which holds no previous artifacts"
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    [ "${entry##*/}" != "${2:-}" ] || continue
    [ ! -d "$entry" ] || refuse "which holds directory ${entry##*/}"
    case ${entry##*/} in
      SHA256SUMS | SKILL.md | contract.txt | petal-wit-*.tar.gz | .DS_Store) ;;
      # macOS writes these AppleDouble files beside the artifacts on volumes
      # without extended attributes, such as FAT or SMB.
      ._SHA256SUMS | ._SKILL.md | ._contract.txt | ._petal-wit-*.tar.gz | ._.DS_Store) ;;
      *) refuse "which holds ${entry##*/}, not a release artifact" ;;
    esac
  done
}
check_out "$out"

# Name the archive after the package its WIT declares, so a contract bump
# cannot ship under the previous version.
contract=$(git show "$commit:wit/route/route.wit" | sed -n 's/^package \(bloom:route@[^;]*\);$/\1/p')
if [ -z "$contract" ]; then
  echo "assemble-release-artifacts: HEAD:wit/route/route.wit declares no bloom:route package" >&2
  exit 1
fi
wit=petal-wit-${contract#bloom:route@}
# Ask git itself whether git archive knows --mtime, rather than running an
# archive that could fail for other reasons.
if ! git archive -h 2>&1 | grep -q -- '--mtime'; then
  echo "assemble-release-artifacts: git archive does not support --mtime; this needs git 2.40 or later" >&2
  exit 1
fi
# A subtree archive otherwise uses wall-clock time, so bind it to HEAD's commit
# time. git reads "@<seconds> +0000" exactly, while a bare @<seconds> below
# 100000000 is misread as another date. --no-show-signature keeps a signature
# check out of that value.
archive_time="@$(git log -1 --no-show-signature --format=%ct "$commit") +0000"

# Enter the output before the build and stay in it, so the directory checked is
# the one written: renaming or replacing $out, or any directory above it, from
# here on cannot send a write or a deletion anywhere else.
mkdir -p "$out"
cd -P -- "$out"
# The directory entered must still be the output, reached through the parent
# resolved above: a swap of either since then is refused.
here=$(pwd -P && echo .)
here=${here%?.}
if [ -L "$out" ] || ! [ . -ef "$out" ] ||
  [ "${here%/*}" != "$resolved_parent" ]; then
  refuse "which changed while it was checked"
fi
check_out .
inspect=$(cd "$root" && cargo run --quiet -p bloom-petal-cli -- inspect)
digest=$(printf '%s\n' "$inspect" | awk '/^wit_digest:/ { print $2 }')
if [ -z "$digest" ]; then
  echo "assemble-release-artifacts: petal inspect reported no wit_digest" >&2
  exit 1
fi
(cd "$root" && guard)

# Stage the new artifacts in a hidden subdirectory, so they replace the previous
# ones only once all of them exist. Only validated artifact files are ever
# deleted: a directory with an artifact's name is never recursive-cleanup
# authority.
stage=$(mktemp -d ./.assemble-release-artifacts.XXXXXX)
stage_name=${stage#./}
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
# The pinned settings keep the caller's global and system git config and
# attributes out of the bytes; a repository info/attributes is refused above.
GIT_ATTR_NOSYSTEM=1 git -C "$root" -c core.autocrlf=false -c core.eol=lf \
  -c core.attributesFile=/dev/null -c tar.umask=0002 \
  -c tar.tar.gz.command='git archive gzip' \
  archive --format=tar.gz --mtime="$archive_time" --prefix="$wit/" \
  "$commit:wit/route" >"$stage/$wit.tar.gz"
cat >"$stage/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF
git -C "$root" show "$commit:skills/bloom-petal-development/SKILL.md" >"$stage/SKILL.md"
(cd -P -- "$stage" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)

check_out . "$stage_name"

# From here on, a failure must keep the staged artifacts and say where they are.
incomplete() {
  trap - EXIT
  echo "assemble-release-artifacts: $out may be incomplete; the new artifacts are in $out/$stage_name. Delete that directory, then rerun to replace them" >&2
  exit 1
}
trap incomplete HUP INT TERM
trap - EXIT
# Delete only the names check_out accepted. SHA256SUMS goes last and comes back
# first, so an interrupted replacement still leaves a directory that a rerun
# accepts as a previous run's output.
for entry in ./* ./.[!.]* ./..?*; do
  [ -e "$entry" ] || [ -L "$entry" ] || continue
  case ${entry#./} in
    "$stage_name" | SHA256SUMS) ;;
    SKILL.md | contract.txt | petal-wit-*.tar.gz | .DS_Store) rm -f -- "$entry" || incomplete ;;
    ._SHA256SUMS | ._SKILL.md | ._contract.txt | ._petal-wit-*.tar.gz | ._.DS_Store) rm -f -- "$entry" || incomplete ;;
    *) incomplete ;;
  esac
done
rm -f ./SHA256SUMS || incomplete
for name in SHA256SUMS contract.txt SKILL.md "$wit.tar.gz"; do
  mv -- "$stage/$name" "./$name" || incomplete
done
trap - HUP INT TERM
rmdir "$stage" || echo "assemble-release-artifacts: could not remove the empty staging directory $out/$stage_name" >&2
