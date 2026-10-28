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
dirty=$(git status --porcelain --untracked-files=all
  git ls-files -v | grep '^[a-zS]' || true
  info_attributes=$(git rev-parse --git-path info/attributes)
  [ ! -s "$info_attributes" ] || echo "$info_attributes")
if [ -n "${CI:-}" ] && [ -n "$dirty" ]; then
  fail "CI's working tree differs from HEAD after the earlier checks, so a release would refuse it: $dirty"
fi
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

# The output must be named directly. A final . or .., or a .. in a path whose
# parent does not exist yet, is refused before anything is created, and the
# previous artifacts stay.
for name in "$tmp/dist/." "$tmp/dist/sub/.." "$tmp/dist/missing/../../dist"; do
  status=0
  assemble "$name" 2>"$tmp/dot.err" || status=$?
  [ "$status" = 2 ] && grep -q -e 'name the output directory itself' -e 'does not exist; create it first' "$tmp/dot.err" ||
    fail "the assembler accepted $name as its output: exit $status, $(cat "$tmp/dot.err")"
done
[ ! -e "$tmp/dist/missing" ] || fail "a refused output path created $tmp/dist/missing"
(cd "$tmp/dist" && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "a refused output lost its previous artifacts"

# The caller's git config and global attributes do not change the artifacts.
mkdir -p "$tmp/xdg/git"
printf '* text eol=crlf\n' >"$tmp/xdg/git/attributes"
(
  XDG_CONFIG_HOME=$tmp/xdg
  GIT_CONFIG_PARAMETERS="'core.autocrlf'='true' 'tar.umask'='0022' 'tar.tar.gz.command'='gzip -9n'"
  export XDG_CONFIG_HOME GIT_CONFIG_PARAMETERS
  assemble "$tmp/hostile"
)
cmp "$tmp/dist/SHA256SUMS" "$tmp/hostile/SHA256SUMS" ||
  fail "the caller's git config or attributes changed the release artifacts"

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

# The stubs below replace one command for a single assembler run, in a
# subshell. Each records that it ran, so a test cannot pass by never reaching
# the step it is about.
real_git=$(command -v git)
real_shasum=$(command -v shasum)
real_cargo=$(command -v cargo)
# no_stage_left <what> <output>: the staging directory lives inside the output.
no_stage_left() {
  for entry in "$2"/.assemble-release-artifacts.*; do
    [ ! -e "$entry" ] || fail "$1 left $entry"
  done
}
# remove_stage <output>: clear a staging directory a failed run kept on purpose.
remove_stage() {
  for entry in "$1"/.assemble-release-artifacts.*; do
    [ -d "$entry" ] || continue
    rm -f "$entry"/*
    rmdir "$entry"
  done
}

# A failure after the artifacts are staged keeps the previous ones and leaves
# no staging directory.
mkdir "$tmp/stub"
printf '#!/bin/sh\n: >"%s/stub/ran"\nexit 1\n' "$tmp" >"$tmp/stub/shasum"
chmod +x "$tmp/stub/shasum"
status=0
(PATH=$tmp/stub:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/stub.err" || status=$?
[ -f "$tmp/stub/ran" ] || fail "the failing checksum stub never ran"
[ "$status" != 0 ] || fail "the assembler succeeded although its checksum step failed"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a failure after staging lost the previous artifacts"
no_stage_left "a failure after staging" "$tmp/stale"

# The output is checked again just before the previous artifacts are deleted:
# a file that appears in it during the build is refused, not deleted.
mkdir "$tmp/appear"
printf '#!/bin/sh\nprintf x >"%s/stale/notes.txt"\nexec "%s" "$@"\n' "$tmp" "$real_shasum" >"$tmp/appear/shasum"
chmod +x "$tmp/appear/shasum"
status=0
(PATH=$tmp/appear:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/appear.err" || status=$?
[ "$status" = 1 ] && grep -q 'holds notes.txt' "$tmp/appear.err" ||
  fail "a file that appeared during the build was not refused: exit $status, $(cat "$tmp/appear.err")"
[ -f "$tmp/stale/notes.txt" ] || fail "a file that appeared during the build was deleted"
rm -f "$tmp/stale/notes.txt"
(cd "$tmp/stale" && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "a refusal just before deletion lost the previous artifacts"
no_stage_left "a refusal just before deletion" "$tmp/stale"

# Renaming the output or replacing it, or a directory above it, with a
# symbolic link cannot redirect the assembler: it enters the output before the
# build and keeps writing to the directory it checked, and the other directory
# is left alone.
mkdir -p "$tmp/anc/out" "$tmp/anc-victim/out" "$tmp/stub-ancestor"
cp "$tmp/dist"/* "$tmp/anc/out/"
printf 'previous\n' >"$tmp/anc-victim/out/contract.txt"
printf 'previous sums\n' >"$tmp/anc-victim/out/SHA256SUMS"
printf '#!/bin/sh\nif [ ! -e "%s/anc.real" ]; then\n  mv "%s/anc" "%s/anc.real"\n  ln -s anc-victim "%s/anc"\nfi\nexec "%s" "$@"\n' \
  "$tmp" "$tmp" "$tmp" "$tmp" "$real_cargo" >"$tmp/stub-ancestor/cargo"
chmod +x "$tmp/stub-ancestor/cargo"
status=0
(PATH=$tmp/stub-ancestor:$PATH; export PATH; assemble "$tmp/anc/out") 2>"$tmp/swap.err" || status=$?
[ -e "$tmp/anc.real" ] || fail "the ancestor swap stub never ran"
[ "$status" = 0 ] && cmp "$tmp/anc.real/out/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a swap of the output's parent during the build moved the artifacts: exit $status, $(cat "$tmp/swap.err")"
[ "$(ls -A "$tmp/anc-victim/out" | LC_ALL=C sort | tr '\n' ' ')" = "SHA256SUMS contract.txt " ] &&
  [ "$(cat "$tmp/anc-victim/out/SHA256SUMS")" = "previous sums" ] ||
  fail "a swap of the output's parent during the build redirected the assembler into another directory"
for when in build export staging; do
  mkdir "$tmp/swap-$when" "$tmp/victim-$when" "$tmp/stub-$when"
  # A previous run's artifacts, so the deletion step runs too.
  cp "$tmp/dist"/* "$tmp/swap-$when/"
  printf 'previous\n' >"$tmp/victim-$when/contract.txt"
  printf 'previous sums\n' >"$tmp/victim-$when/SHA256SUMS"
  case $when in
    build) command=cargo real=$real_cargo only= ;;
    # git show exports SKILL.md into the stage after the build and before the
    # stage is checksummed.
    export) command=git real=$real_git only='[ "$3" = show ] && ' ;;
    staging) command=shasum real=$real_shasum only= ;;
  esac
  printf '#!/bin/sh\nif %s[ ! -e "%s/swap-%s.real" ]; then\n  mv "%s/swap-%s" "%s/swap-%s.real"\n  ln -s "victim-%s" "%s/swap-%s"\nfi\nexec "%s" "$@"\n' \
    "$only" "$tmp" "$when" "$tmp" "$when" "$tmp" "$when" "$when" "$tmp" "$when" "$real" >"$tmp/stub-$when/$command"
  chmod +x "$tmp/stub-$when/$command"
  status=0
  (PATH=$tmp/stub-$when:$PATH; export PATH; assemble "$tmp/swap-$when") 2>"$tmp/swap.err" || status=$?
  [ -e "$tmp/swap-$when.real" ] || fail "the $when swap stub never ran"
  [ "$(ls -A "$tmp/victim-$when" | LC_ALL=C sort | tr '\n' ' ')" = "SHA256SUMS contract.txt " ] &&
    [ "$(cat "$tmp/victim-$when/contract.txt")" = previous ] &&
    [ "$(cat "$tmp/victim-$when/SHA256SUMS")" = "previous sums" ] ||
    fail "swapping the output during $when redirected the assembler into another directory"
  no_stage_left "a swap during $when" "$tmp/victim-$when"
  [ "$status" = 0 ] || fail "a swap during $when failed: exit $status, $(cat "$tmp/swap.err")"
  cmp "$tmp/swap-$when.real/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
    fail "a swap during $when kept the artifacts out of the directory that was checked"
done

# Between the first check and entering the output, the script still names the
# output by its path. A swap of the output, or of its parent, in that window
# is noticed once the script is inside, and refused; the other directory, made
# to look like a previous run's output, is left alone.
for what in output parent; do
  mkdir -p "$tmp/win-$what/out" "$tmp/winvictim-$what/out" "$tmp/winstub-$what"
  printf 'previous\n' >"$tmp/winvictim-$what/out/contract.txt"
  printf 'previous sums\n' >"$tmp/winvictim-$what/out/SHA256SUMS"
  case $what in
    output) from=$tmp/win-$what/out to=$tmp/winvictim-$what/out ;;
    parent) from=$tmp/win-$what to=$tmp/winvictim-$what ;;
  esac
  # git log reads the commit time, after the first check and before the
  # script enters the output.
  printf '#!/bin/sh\nif [ "$1" = log ] && [ ! -e "%s.real" ]; then\n  mv "%s" "%s.real"\n  ln -s "%s" "%s"\nfi\nexec "%s" "$@"\n' \
    "$from" "$from" "$from" "$to" "$from" "$real_git" >"$tmp/winstub-$what/git"
  chmod +x "$tmp/winstub-$what/git"
  status=0
  (PATH=$tmp/winstub-$what:$PATH; export PATH; assemble "$tmp/win-$what/out") 2>"$tmp/win.err" || status=$?
  [ -e "$from.real" ] || fail "the $what swap stub never ran"
  [ "$status" = 1 ] && grep -q 'changed while it was checked' "$tmp/win.err" ||
    fail "a swap of the $what before the script entered it was not refused: exit $status, $(cat "$tmp/win.err")"
  [ "$(ls -A "$tmp/winvictim-$what/out" | LC_ALL=C sort | tr '\n' ' ')" = "SHA256SUMS contract.txt " ] &&
    [ "$(cat "$tmp/winvictim-$what/out/SHA256SUMS")" = "previous sums" ] ||
    fail "a swap of the $what before the script entered it redirected the assembler"
done

# An output whose parent does not exist yet is refused before anything is
# created, so no parent can be planted as a symbolic link while the script
# works.
status=0
assemble "$tmp/win-new/out" 2>"$tmp/win.err" || status=$?
[ "$status" = 2 ] && grep -q 'does not exist; create it first' "$tmp/win.err" ||
  fail "an output whose parent does not exist was not refused: exit $status, $(cat "$tmp/win.err")"
[ ! -e "$tmp/win-new" ] || fail "a refused output whose parent does not exist created $tmp/win-new"

# A parent whose name ends in a newline is kept intact when it is resolved.
nl='
'
mkdir "$tmp/nl$nl"
assemble "$tmp/nl$nl/out"
cmp "$tmp/nl$nl/out/SHA256SUMS" "$tmp/dist/SHA256SUMS" && [ ! -e "$tmp/nl" ] ||
  fail "an output under a parent whose name ends in a newline was written elsewhere"

# A failure while the new artifacts are moved in keeps the rest in the staging
# directory and says so. SHA256SUMS comes back first, so the output still reads
# as a previous run's: deleting the staging directory and rerunning recovers it.
mkdir "$tmp/mvstub"
printf '#!/bin/sh\nif [ -e "%s/mvstub/first" ]; then\n  : >"%s/mvstub/ran"\n  exit 1\nfi\n: >"%s/mvstub/first"\nexec "%s" "$@"\n' \
  "$tmp" "$tmp" "$tmp" "$(command -v mv)" >"$tmp/mvstub/mv"
chmod +x "$tmp/mvstub/mv"
status=0
(PATH=$tmp/mvstub:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/mv.err" || status=$?
[ -f "$tmp/mvstub/ran" ] || fail "the failing mv stub never ran"
[ "$status" = 1 ] && grep -q 'Delete that directory, then rerun' "$tmp/mv.err" ||
  fail "a failure while moving the artifacts in was not reported: exit $status, $(cat "$tmp/mv.err")"
[ -f "$tmp/stale/SHA256SUMS" ] ||
  fail "a failure while moving the artifacts in left the output without SHA256SUMS"
kept=
for entry in "$tmp/stale"/.assemble-release-artifacts.*; do
  [ -d "$entry" ] && kept=$entry
done
[ -n "$kept" ] && [ -f "$kept/contract.txt" ] ||
  fail "a failure while moving the artifacts in did not keep them"
status=0
assemble "$tmp/stale" 2>"$tmp/rerun.err" || status=$?
[ "$status" = 1 ] && grep -q 'delete it and rerun' "$tmp/rerun.err" ||
  fail "a rerun with the kept staging directory did not say how to recover: exit $status, $(cat "$tmp/rerun.err")"
remove_stage "$tmp/stale"
assemble "$tmp/stale"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a rerun did not recover from a failure while moving the artifacts in"

# A failure while the previous artifacts are deleted keeps SHA256SUMS, so
# deleting the staging directory and rerunning recovers the output.
mkdir "$tmp/rmstub"
printf '#!/bin/sh\ncase "$*" in\n  */contract.txt) : >"%s/rmstub/ran"; exit 1 ;;\nesac\nexec "%s" "$@"\n' "$tmp" "$(command -v rm)" >"$tmp/rmstub/rm"
chmod +x "$tmp/rmstub/rm"
status=0
(PATH=$tmp/rmstub:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/rm.err" || status=$?
[ -f "$tmp/rmstub/ran" ] || fail "the failing rm stub never ran"
[ "$status" = 1 ] && grep -q 'the new artifacts are in' "$tmp/rm.err" ||
  fail "a failure while deleting the previous artifacts was not reported: exit $status, $(cat "$tmp/rm.err")"
[ -f "$tmp/stale/SHA256SUMS" ] ||
  fail "a failure while deleting the previous artifacts removed SHA256SUMS before the others"
status=0
assemble "$tmp/stale" 2>"$tmp/rerun.err" || status=$?
[ "$status" = 1 ] && grep -q 'delete it and rerun' "$tmp/rerun.err" ||
  fail "a rerun with the kept staging directory did not say how to recover: exit $status, $(cat "$tmp/rerun.err")"
remove_stage "$tmp/stale"
assemble "$tmp/stale"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a rerun did not recover from a failure while deleting the previous artifacts"

# An output this user cannot read or write is refused, not half-replaced.
# root can read and write anything, so this only runs as another user.
if [ "$(id -u)" != 0 ]; then
  for mode in 300 500 600; do
    mkdir "$tmp/locked-$mode"
    chmod "$mode" "$tmp/locked-$mode"
    status=0
    assemble "$tmp/locked-$mode" 2>"$tmp/locked.err" || status=$?
    chmod 700 "$tmp/locked-$mode"
    [ "$status" = 1 ] && grep -q 'cannot read and write' "$tmp/locked.err" ||
      fail "an output with mode $mode was not refused: exit $status, $(cat "$tmp/locked.err")"
  done
fi

# A git without git archive --mtime is refused before anything is built, with
# a message that names the version it needs.
mkdir "$tmp/oldgit"
printf '#!/bin/sh\nif [ "$1" = archive ] && [ "$2" = -h ]; then\n  : >"%s/oldgit/ran"\n  echo "usage: git archive [<options>] <tree-ish> [<path>...]"\n  exit 129\nfi\nexec "%s" "$@"\n' "$tmp" "$real_git" >"$tmp/oldgit/git"
chmod +x "$tmp/oldgit/git"
status=0
(PATH=$tmp/oldgit:$PATH; export PATH; assemble "$tmp/oldgit-out") 2>"$tmp/oldgit.err" || status=$?
[ -f "$tmp/oldgit/ran" ] || fail "the old-git stub never ran"
[ "$status" = 1 ] && grep -q 'needs git 2.40 or later' "$tmp/oldgit.err" ||
  fail "a git without archive --mtime was not refused: exit $status, $(cat "$tmp/oldgit.err")"
[ ! -e "$tmp/oldgit-out" ] || fail "a refused old git still created $tmp/oldgit-out"
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
  echo "test-release-artifacts: the working tree differs from HEAD, or the repository has an info/attributes; contract.txt's wit_digest may not describe the archived WIT" >&2

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

# Without --allow-dirty, the assembler refuses a working tree that differs from
# HEAD anywhere (the CLI source included), that hides changes from git, or
# whose repository info/attributes would change the archive, before it touches
# a previous run's artifacts; it also refuses a symbolic link as its output,
# with or without a trailing slash. This runs it in a scratch repository, so
# each refusal comes before it needs cargo. The subshell drops the caller's git
# environment, config and attributes: run from a hook or `git rebase -x`,
# GIT_DIR and GIT_INDEX_FILE would otherwise point these commands at the
# caller's repository.
(
  # git rev-parse --local-env-vars does not list GIT_ATTR_SOURCE.
  unset $(git rev-parse --local-env-vars) GIT_ATTR_SOURCE
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_ATTR_NOSYSTEM=1
  repo=$tmp/guard
  mkdir -p "$repo/scripts" "$repo/wit/route" "$repo/src" "$repo/prev" \
    "$repo/crates/petal-cli/src" "$repo/skills/bloom-petal-development"
  cp scripts/assemble-release-artifacts.sh "$repo/scripts/"
  printf 'package bloom:route@0.0.0;\n' >"$repo/wit/route/route.wit"
  : >"$repo/src/lib.rs"
  : >"$repo/crates/petal-cli/src/main.rs"
  printf 'skill\n' >"$repo/skills/bloom-petal-development/SKILL.md"
  printf 'previous\n' >"$repo/prev/contract.txt"
  : >"$repo/prev/SHA256SUMS"
  git -C "$repo" init -q --template=
  git -C "$repo" config core.excludesFile /dev/null
  git -C "$repo" config core.attributesFile /dev/null
  git -C "$repo" config user.name test
  git -C "$repo" config user.email test@example.invalid
  git -C "$repo" config commit.gpgsign false
  git -C "$repo" add scripts wit src crates skills
  git -C "$repo" commit -qm init
  # prev/ is an untracked output inside the repository, which the guard
  # leaves out; every refusal below is about the change that case makes.
  mkdir -p "$repo/.git/info"
  for change in modified cli staged untracked untracked-hidden assume-unchanged skip-worktree; do
    case $change in
      modified) printf '// change\n' >>"$repo/wit/route/route.wit" ;;
      cli) printf '// change\n' >>"$repo/crates/petal-cli/src/main.rs" ;;
      staged) printf '// change\n' >>"$repo/src/lib.rs" && git -C "$repo" add src/lib.rs ;;
      untracked-hidden) git -C "$repo" config status.showUntrackedFiles no && printf 'fn main() {}\n' >"$repo/crates/petal-cli/build.rs" ;;
      untracked) mkdir -p "$repo/wit/route/deps/new" && printf 'package a:b;\n' >"$repo/wit/route/deps/new/new.wit" ;;
      assume-unchanged) git -C "$repo" update-index --assume-unchanged wit/route/route.wit && printf '// change\n' >>"$repo/wit/route/route.wit" ;;
      skip-worktree) git -C "$repo" update-index --skip-worktree crates/petal-cli/src/main.rs && printf '// change\n' >>"$repo/crates/petal-cli/src/main.rs" ;;
    esac
    status=0
    "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
    [ "$status" = 1 ] && grep -q -e 'the working tree differs from HEAD' -e 'git is told to ignore changes to' "$tmp/guard.err" ||
      fail "the assembler did not refuse a $change input: exit $status, $(cat "$tmp/guard.err")"
    [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
      fail "the assembler replaced the previous artifacts despite a $change input"
    git -C "$repo" update-index --no-assume-unchanged wit/route/route.wit
    git -C "$repo" update-index --no-skip-worktree crates/petal-cli/src/main.rs
    git -C "$repo" restore --source=HEAD --staged --worktree -- wit/route/route.wit src/lib.rs crates/petal-cli/src/main.rs
    git -C "$repo" config --unset status.showUntrackedFiles || true
    rm -f "$repo/crates/petal-cli/build.rs"
    if [ -f "$repo/wit/route/deps/new/new.wit" ]; then
      rm -f "$repo/wit/route/deps/new/new.wit"
      rmdir "$repo/wit/route/deps/new" "$repo/wit/route/deps"
    fi
  done
  # An output named like a pathspec glob is excluded literally, so it hides
  # neither prev/ nor the WIT from the guard.
  printf '// change\n' >>"$repo/wit/route/route.wit"
  status=0
  "$repo/scripts/assemble-release-artifacts.sh" "$repo/[pw]*" 2>"$tmp/guard.err" || status=$?
  [ "$status" = 1 ] && grep -q 'wit/route/route.wit' "$tmp/guard.err" ||
    fail "an output named [pw]* hid a modified WIT from the guard: exit $status, $(cat "$tmp/guard.err")"
  git -C "$repo" restore -- wit/route/route.wit
  # Reached through a symbolic link to the repository, an output inside it is
  # still recognised as inside, so the guard still leaves it out.
  ln -s guard "$tmp/guard-link"
  status=0
  "$tmp/guard-link/scripts/assemble-release-artifacts.sh" "$tmp/guard-link/prev" 2>"$tmp/guard.err" || status=$?
  ! grep -q -e refusing -e 'differs from HEAD' -e 'told to ignore' "$tmp/guard.err" ||
    fail "an output reached through a symbolic link to the repository was not left out: $(cat "$tmp/guard.err")"
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts when reached through a symbolic link"
  # A caller's GIT_LITERAL_PATHSPECS does not switch off that exclusion.
  status=0
  GIT_LITERAL_PATHSPECS=1 "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
  ! grep -q -e refusing -e 'differs from HEAD' -e 'told to ignore' "$tmp/guard.err" ||
    fail "GIT_LITERAL_PATHSPECS stopped the guard leaving out an output inside the repository: $(cat "$tmp/guard.err")"
  printf '*.wit export-ignore\n' >"$repo/.git/info/attributes"
  status=0
  "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
  [ "$status" = 1 ] && grep -q 'info/attributes' "$tmp/guard.err" ||
    fail "the assembler did not refuse a repository info/attributes: exit $status, $(cat "$tmp/guard.err")"
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts despite a repository info/attributes"
  rm -f "$repo/.git/info/attributes"
  # A replace ref is ignored: the assembler reads HEAD's own WIT, so it gets
  # past the contract check and stops only where it needs cargo.
  blob=$(git -C "$repo" rev-parse HEAD:wit/route/route.wit)
  git -C "$repo" replace "$blob" "$(printf 'replaced\n' | git -C "$repo" hash-object -w --stdin)"
  status=0
  "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev" 2>"$tmp/guard.err" || status=$?
  ! grep -q 'declares no bloom:route package' "$tmp/guard.err" ||
    fail "the assembler read a replaced WIT object instead of HEAD's"
  ! grep -q -e refusing -e 'differs from HEAD' -e 'told to ignore' "$tmp/guard.err" ||
    fail "the guard refused a clean tree with an untracked output inside it: $(cat "$tmp/guard.err")"
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts despite a replace ref"
  git -C "$repo" replace -d "$blob" >/dev/null
  mkdir "$tmp/headmove"
  printf '#!/bin/sh\n: >"%s/headmove/ran"\ngit commit -q --allow-empty -m moved\necho "wit_digest: 0"\n' "$tmp" >"$tmp/headmove/cargo"
  chmod +x "$tmp/headmove/cargo"
  status=0
  (PATH=$tmp/headmove:$PATH; export PATH; "$repo/scripts/assemble-release-artifacts.sh" "$repo/prev") 2>"$tmp/guard.err" || status=$?
  [ -f "$tmp/headmove/ran" ] || fail "the HEAD-moving cargo stub never ran"
  [ "$status" = 1 ] && grep -q 'HEAD moved' "$tmp/guard.err" ||
    fail "the assembler did not refuse a HEAD that moved during the build: exit $status, $(cat "$tmp/guard.err")"
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts although HEAD moved"
  git -C "$repo" reset -q --soft HEAD~1
  # Every read names the commit HEAD pointed at when the assembler started,
  # so a commit made during the build cannot change what is archived.
  printf '#!/bin/sh\n: >"%s/headmove/pinned"\nprintf "package bloom:route@9.9.9;\\n" >wit/route/route.wit\nprintf "moved\\n" >skills/bloom-petal-development/SKILL.md\ngit commit -qam moved\necho "wit_digest: 0"\n' "$tmp" >"$tmp/headmove/cargo"
  started=$(git -C "$repo" rev-parse HEAD)
  status=0
  (PATH=$tmp/headmove:$PATH; export PATH; "$repo/scripts/assemble-release-artifacts.sh" --allow-dirty "$tmp/pinned") 2>"$tmp/guard.err" || status=$?
  [ -f "$tmp/headmove/pinned" ] || fail "the committing cargo stub never ran"
  [ "$status" = 0 ] && grep -qx "source_commit: $started" "$tmp/pinned/contract.txt" &&
    tar -xzOf "$tmp/pinned/petal-wit-0.0.0.tar.gz" petal-wit-0.0.0/route.wit | grep -qx 'package bloom:route@0.0.0;' &&
    [ "$(cat "$tmp/pinned/SKILL.md")" = skill ] ||
    fail "a commit made during the build changed what was archived: exit $status, $(cat "$tmp/guard.err")"
  git -C "$repo" reset -q --soft "$started"
  git -C "$repo" restore --source=HEAD --staged --worktree -- wit/route/route.wit skills/bloom-petal-development/SKILL.md
  # The symbolic link is itself an untracked file, so these runs skip the
  # working-tree guard with --allow-dirty; the output checks still apply.
  ln -s prev "$repo/link"
  for name in link link/; do
    status=0
    "$repo/scripts/assemble-release-artifacts.sh" --allow-dirty "$repo/$name" 2>"$tmp/guard.err" || status=$?
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

# An output holding artifact names but no SHA256SUMS, such as another skill's
# directory, is not a previous run's output: it is refused and kept.
mkdir "$tmp/nosums"
printf 'mine\n' >"$tmp/nosums/SKILL.md"
status=0
assemble "$tmp/nosums" 2>"$tmp/nosums.err" || status=$?
[ "$status" = 1 ] && grep -q 'holds no previous artifacts' "$tmp/nosums.err" ||
  fail "an output without SHA256SUMS was not refused: exit $status, $(cat "$tmp/nosums.err")"
[ "$(cat "$tmp/nosums/SKILL.md")" = mine ] ||
  fail "a refused output without SHA256SUMS lost its SKILL.md"
