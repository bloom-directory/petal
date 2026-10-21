#!/bin/sh
# Assemble the release artifacts for HEAD into a directory (default: dist at
# the repository root). A relative directory is taken from the caller's
# working directory. It must name the directory itself: it may not end in . or
# .., and may contain .. only when its parent directory already exists.
#
# The directory is replaced, so it must be new, empty, or hold only a previous
# run's artifacts; it may not be a symbolic link, and this user must be able to
# read and write it. The artifacts are written beside it and moved in at the
# end, so its parent must be writable too. A failure before the replacement
# leaves a previous run's artifacts as they were; a failure while they are
# replaced names the directory that holds the new ones, and a rerun replaces
# them.
#
# The WIT archive takes its timestamps from HEAD's commit time, which needs git
# 2.40 or later.
#
# contract.txt's wit_digest comes from the working tree's contract crate and
# the archive from HEAD, so the script refuses to run while wit/ or src/
# differs from HEAD, untracked files included, or holds files git is told to
# ignore changes to. It also refuses a repository info/attributes file, which
# git archive always applies. --allow-dirty skips these checks, as it does for
# cargo package; only tests use it.
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
# A final . or .. would put the staging directory inside the output.
case $out in
  */. | */..)
    echo "assemble-release-artifacts: name the output directory itself, not $out" >&2
    exit 2
    ;;
esac
# Resolve the parent when it exists, so a .. inside the path cannot make the
# staging step create a directory inside the output. A parent that does not
# exist yet may not be reached through .. at all.
dir=${out%/*}
dir=${dir:-/}
if [ -d "$dir" ]; then
  # The trailing . keeps a parent whose name ends in a newline intact.
  resolved=$(CDPATH= cd -P -- "$dir" && pwd -P && echo .) || {
    echo "assemble-release-artifacts: cannot enter $dir, the parent of $out" >&2
    exit 1
  }
  resolved=${resolved%?.}
  out=${resolved%/}/${out##*/}
else
  case $out in
    */../*)
      echo "assemble-release-artifacts: $out passes through .., so its parent $dir must already exist" >&2
      exit 2
      ;;
  esac
fi
cd "$root"
# A replace ref would make git archive and git show read objects other than
# HEAD's, unseen by git status, so ignore replace refs throughout.
GIT_NO_REPLACE_OBJECTS=1
export GIT_NO_REPLACE_OBJECTS

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
  # git archive always applies the repository's own info/attributes, and no
  # option turns that off, so it could drop or rewrite archived WIT files.
  info_attributes=$(git rev-parse --git-path info/attributes)
  if [ -s "$info_attributes" ]; then
    echo "assemble-release-artifacts: $info_attributes would change the WIT archive; move it aside" >&2
    exit 1
  fi
fi

refuse() {
  echo "assemble-release-artifacts: refusing to replace $out, $*" >&2
  exit 1
}
# The output must be a real directory holding nothing, or only a previous
# run's artifacts. This runs before the build and again just before the
# previous artifacts are replaced.
check_out() {
  if [ -L "$out" ]; then
    refuse "which is a symbolic link"
  fi
  if [ -e "$out" ] && [ ! -d "$out" ]; then
    refuse "which is not a directory"
  fi
  [ -e "$out" ] || return 0
  if [ ! -r "$out" ] || [ ! -w "$out" ] || [ ! -x "$out" ]; then
    refuse "which this user cannot read and write"
  fi
  listing=$(ls -A -- "$out") || refuse "which cannot be listed"
  if [ -n "$listing" ]; then
    [ -f "$out/SHA256SUMS" ] || refuse "which holds no previous artifacts"
    for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
      [ -e "$entry" ] || [ -L "$entry" ] || continue
      [ ! -d "$entry" ] || refuse "which holds directory ${entry##*/}"
      case ${entry##*/} in
        SHA256SUMS | SKILL.md | contract.txt | petal-wit-*.tar.gz | .DS_Store) ;;
        # macOS writes these AppleDouble files beside the artifacts on volumes
        # without extended attributes, such as FAT or SMB.
        ._SHA256SUMS | ._SKILL.md | ._contract.txt | ._petal-wit-*.tar.gz | ._.DS_Store) ;;
        *) refuse "which holds ${entry##*/}, not a release artifact" ;;
      esac
    done
  fi
}
check_out

# Name the archive after the package its WIT declares, so a contract bump
# cannot ship under the previous version.
contract=$(git show HEAD:wit/route/route.wit | sed -n 's/^package \(bloom:route@[^;]*\);$/\1/p')
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
parent=${out%/*}
parent=${parent:-/}
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
# A subtree archive otherwise uses wall-clock time, so bind it to HEAD's commit
# time. git reads "@<seconds> +0000" exactly, while a bare @<seconds> below
# 100000000 is misread as another date. --no-show-signature keeps a signature
# check out of that value.
# The pinned settings keep the caller's global and system git config and
# attributes out of the bytes; a repository info/attributes is refused above.
archive_time="@$(git log -1 --no-show-signature --format=%ct HEAD) +0000"
GIT_ATTR_NOSYSTEM=1 git -c core.autocrlf=false -c core.eol=lf \
  -c core.attributesFile=/dev/null -c tar.umask=0002 \
  -c tar.tar.gz.command='git archive gzip' \
  archive --format=tar.gz --mtime="$archive_time" --prefix="$wit/" \
  -o "$stage/$wit.tar.gz" HEAD:wit/route
cat >"$stage/contract.txt" <<EOF
source_commit: $commit
contract: $contract
wit_digest: $digest
EOF
git show HEAD:skills/bloom-petal-development/SKILL.md >"$stage/SKILL.md"
(cd "$stage" && shasum -a 256 contract.txt "$wit.tar.gz" SKILL.md >SHA256SUMS)

check_out

# From here on, a failure must keep the staged artifacts and say where they are.
incomplete() {
  trap - EXIT
  echo "assemble-release-artifacts: $out may be incomplete; the new artifacts are in $stage. Rerun to replace them, then remove $stage" >&2
  exit 1
}
trap incomplete HUP INT TERM
trap - EXIT
# Delete only the names check_out accepted. SHA256SUMS goes last and comes back
# first, so an interrupted replacement still leaves a directory that a rerun
# accepts as a previous run's output.
if [ -d "$out" ]; then
  for entry in "$out"/* "$out"/.[!.]* "$out"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    case ${entry##*/} in
      SHA256SUMS) ;;
      SKILL.md | contract.txt | petal-wit-*.tar.gz | .DS_Store) rm -f -- "$entry" || incomplete ;;
      ._SHA256SUMS | ._SKILL.md | ._contract.txt | ._petal-wit-*.tar.gz | ._.DS_Store) rm -f -- "$entry" || incomplete ;;
      *) incomplete ;;
    esac
  done
  rm -f -- "$out/SHA256SUMS" || incomplete
fi
mkdir -p "$out" || incomplete
for name in SHA256SUMS contract.txt SKILL.md "$wit.tar.gz"; do
  mv -- "$stage/$name" "$out/$name" || incomplete
done
trap - HUP INT TERM
rmdir "$stage" || echo "assemble-release-artifacts: could not remove the empty staging directory $stage" >&2
