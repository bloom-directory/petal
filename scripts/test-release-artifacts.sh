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
  [ "$status" = 2 ] && grep -q -e 'name the output directory itself' -e 'must already exist' "$tmp/dot.err" ||
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

# Renaming the output or replacing it with a symbolic link cannot redirect the
# assembler. During the build, before it enters the output, the swap is
# noticed and refused; once it is inside, it keeps writing to the directory it
# checked. Either way the other directory is left alone.
for when in build staging; do
  mkdir "$tmp/swap-$when" "$tmp/victim-$when" "$tmp/stub-$when"
  printf 'previous\n' >"$tmp/victim-$when/contract.txt"
  : >"$tmp/victim-$when/SHA256SUMS"
  case $when in
    build) command=cargo real=$real_cargo ;;
    staging) command=shasum real=$real_shasum ;;
  esac
  printf '#!/bin/sh\nif [ ! -e "%s/swap-%s.real" ]; then\n  mv "%s/swap-%s" "%s/swap-%s.real"\n  ln -s "victim-%s" "%s/swap-%s"\nfi\nexec "%s" "$@"\n' \
    "$tmp" "$when" "$tmp" "$when" "$tmp" "$when" "$when" "$tmp" "$when" "$real" >"$tmp/stub-$when/$command"
  chmod +x "$tmp/stub-$when/$command"
  status=0
  (PATH=$tmp/stub-$when:$PATH; export PATH; assemble "$tmp/swap-$when") 2>"$tmp/swap.err" || status=$?
  [ -e "$tmp/swap-$when.real" ] || fail "the $when swap stub never ran"
  [ "$(cat "$tmp/victim-$when/contract.txt")" = previous ] &&
    [ ! -e "$tmp/victim-$when/SKILL.md" ] ||
    fail "swapping the output during $when redirected the assembler into another directory"
  no_stage_left "a swap during $when" "$tmp/victim-$when"
  case $when in
    build)
      [ "$status" = 1 ] && grep -q 'changed while the artifacts were built' "$tmp/swap.err" ||
        fail "a swap during the build was not refused: exit $status, $(cat "$tmp/swap.err")"
      ;;
    staging)
      [ "$status" = 0 ] || fail "a swap after entering the output failed: exit $status, $(cat "$tmp/swap.err")"
      cmp "$tmp/swap-$when.real/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
        fail "a swap after entering the output kept the artifacts out of the directory that was checked"
      ;;
  esac
done

# A failure while the new artifacts are moved in keeps them in the staging
# directory and says so, and a rerun recovers the output.
mkdir "$tmp/mvstub"
printf '#!/bin/sh\n: >"%s/mvstub/ran"\nexit 1\n' "$tmp" >"$tmp/mvstub/mv"
chmod +x "$tmp/mvstub/mv"
status=0
(PATH=$tmp/mvstub:$PATH; export PATH; assemble "$tmp/stale") 2>"$tmp/mv.err" || status=$?
[ -f "$tmp/mvstub/ran" ] || fail "the failing mv stub never ran"
[ "$status" = 1 ] && grep -q 'the new artifacts are in' "$tmp/mv.err" ||
  fail "a failure while moving the artifacts in was not reported: exit $status, $(cat "$tmp/mv.err")"
kept=
for entry in "$tmp/stale"/.assemble-release-artifacts.*; do
  [ -d "$entry" ] && kept=$entry
done
[ -n "$kept" ] && (cd "$kept" && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "a failure while moving the artifacts in did not keep them"
remove_stage "$tmp/stale"
assemble "$tmp/stale"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a rerun did not recover from a failure while moving the artifacts in"

# A failure while the previous artifacts are deleted keeps SHA256SUMS, so a
# rerun still accepts the output and recovers it.
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
remove_stage "$tmp/stale"
assemble "$tmp/stale"
cmp "$tmp/stale/SHA256SUMS" "$tmp/dist/SHA256SUMS" ||
  fail "a rerun did not recover from a failure while deleting the previous artifacts"

# An output this user cannot read or write is refused, not half-replaced.
# root can read and write anything, so this only runs as another user.
if [ "$(id -u)" != 0 ]; then
  for mode in 300 500; do
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
    "$repo/crates/petal-cli/src"
  cp scripts/assemble-release-artifacts.sh "$repo/scripts/"
  printf 'package bloom:route@0.0.0;\n' >"$repo/wit/route/route.wit"
  : >"$repo/src/lib.rs"
  : >"$repo/crates/petal-cli/src/main.rs"
  printf 'previous\n' >"$repo/prev/contract.txt"
  : >"$repo/prev/SHA256SUMS"
  git -C "$repo" init -q --template=
  git -C "$repo" config core.excludesFile /dev/null
  git -C "$repo" config core.attributesFile /dev/null
  git -C "$repo" config user.name test
  git -C "$repo" config user.email test@example.invalid
  git -C "$repo" config commit.gpgsign false
  git -C "$repo" add scripts wit src crates
  git -C "$repo" commit -qm init
  # prev/ is an untracked output inside the repository, which the guard
  # leaves out; every refusal below is about the change that case makes.
  mkdir -p "$repo/.git/info"
  for change in modified cli staged untracked assume-unchanged skip-worktree; do
    case $change in
      modified) printf '// change\n' >>"$repo/wit/route/route.wit" ;;
      cli) printf '// change\n' >>"$repo/crates/petal-cli/src/main.rs" ;;
      staged) printf '// change\n' >>"$repo/src/lib.rs" && git -C "$repo" add src/lib.rs ;;
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
    if [ -f "$repo/wit/route/deps/new/new.wit" ]; then
      rm -f "$repo/wit/route/deps/new/new.wit"
      rmdir "$repo/wit/route/deps/new" "$repo/wit/route/deps"
    fi
  done
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
  [ "$(cat "$repo/prev/contract.txt")" = previous ] ||
    fail "the assembler replaced the previous artifacts despite a replace ref"
  git -C "$repo" replace -d "$blob" >/dev/null
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
