#!/bin/sh
# Run the petal CLI of the SDK this Petal pins.
#
# route/Cargo.toml names the SDK release tag, or a commit, and
# route/Cargo.lock records the commit. The first time it sees that pin, this
# checks it: a tag must name the commit, and a commit must be on the SDK
# repository's master or a release tag. Then it installs the petal CLI from
# the commit, with the wasm-tools release it builds with, under
# ~/.cache/bloom-petal. So the SDK pin is the only version to change.
# PETAL_BIN names another binary, as the release workflow does.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

if [ -n "${PETAL_BIN:-}" ]; then
  exec "$PETAL_BIN" "$@"
fi

lock=route/Cargo.lock
if [ ! -f "$lock" ]; then
  echo "petal.sh: $lock is missing; create it with: cargo test --manifest-path route/Cargo.toml" >&2
  exit 1
fi
source=$(awk '
  /^\[\[package\]\]/ { sdk = 0 }
  $0 == "name = \"bloom-petal-sdk\"" { sdk = 1 }
  sdk && /^source = / { sub(/^source = "/, ""); sub(/"$/, ""); print; exit }
' "$lock")
repo=https://github.com/bloom-directory/petal
tag=
case "$source" in
  "git+$repo?tag="*"#"* | "git+$repo.git?tag="*"#"*)
    pin=${source#*\?tag=}
    tag=${pin%%#*}
    commit=${pin##*#}
    ;;
  "git+$repo?rev="*"#"* | "git+$repo.git?rev="*"#"*)
    commit=${source##*#}
    ;;
  "")
    # A Cargo config [patch] puts the SDK at a local checkout: use the petal
    # CLI on PATH, built from that checkout.
    exec petal "$@"
    ;;
  *)
    echo "petal.sh: $lock takes bloom-petal-sdk from $source; pin a release tag or a commit of $repo, or set PETAL_BIN to a petal CLI you built" >&2
    exit 1
    ;;
esac
# The tag and commit go into a Git ref and a path below, so take only a
# release tag and a full commit.
if [ -n "$tag" ]; then
  case "$tag" in
    v[0-9]*) ;;
    *) commit= ;;
  esac
  case "$tag" in
    *[!0-9A-Za-z.+-]*) commit= ;;
  esac
fi
case "$commit" in
  *[!0-9a-f]*) commit= ;;
esac
if [ "${#commit}" -ne 40 ]; then
  echo "petal.sh: $lock does not pin bloom-petal-sdk to a release tag or a full commit: $source" >&2
  exit 1
fi

cache=${PETAL_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/bloom-petal}
tools=$cache/$commit
# GitHub serves a commit from any fork of the repository at its URL, so run
# only a commit the repository vouches for: the one a tag pin's tag names, or
# for a commit pin, one on master or a release tag. A file per pin records
# that it passed. Without one, check, even with a CLI already here: an earlier
# petal.sh installed without checking, and the CLI may have been installed
# for another pin.
if [ -n "$tag" ]; then
  checked=$tools/checked-tags/$tag
else
  checked=$tools/checked-commit
fi
if [ ! -f "$checked" ]; then
  if [ -n "$tag" ]; then
    if ! refs=$(git ls-remote "$repo" "refs/tags/$tag" "refs/tags/$tag^{}"); then
      echo "petal.sh: could not read $tag from $repo" >&2
      exit 1
    fi
    named=$(printf '%s\n' "$refs" | awk -v ref="refs/tags/$tag" '
      $2 == ref { plain = $1 }
      $2 == ref "^{}" { peeled = $1 }
      END { print (peeled != "" ? peeled : plain) }
    ')
    if [ "$named" != "$commit" ]; then
      echo "petal.sh: $lock pins $tag at $commit, but $repo names ${named:-no commit} for $tag" >&2
      exit 1
    fi
  else
    # The release workflow applies the same rule. A full fetch, not a partial
    # clone, which would fetch a missing commit on demand.
    history=$cache/sdk-history.git
    git init --quiet --bare "$history"
    if ! git -C "$history" fetch --quiet --no-tags "$repo" \
      '+refs/heads/master:refs/heads/master' '+refs/tags/v*:refs/tags/v*'; then
      echo "petal.sh: could not read master and the release tags from $repo" >&2
      exit 1
    fi
    if ! git -C "$history" merge-base --is-ancestor "$commit" refs/heads/master 2>/dev/null &&
      [ -z "$(git -C "$history" for-each-ref --contains "$commit" 'refs/tags/v[0-9]*' 2>/dev/null)" ]; then
      echo "petal.sh: $lock pins commit $commit, which is not on master or a release tag of $repo; pin one that is, or set PETAL_BIN to a petal CLI you built" >&2
      exit 1
    fi
  fi
  if [ ! -x "$tools/bin/petal" ]; then
    echo "petal.sh: installing the petal CLI at $commit into $tools" >&2
    cargo install --locked --quiet --git "$repo" --rev "$commit" --root "$tools" bloom-petal-cli
  fi
  mkdir -p "${checked%/*}"
  : >"$checked"
fi

# Build with the wasm-tools release this CLI is tested with.
wasm_tools=$("$tools/bin/petal" inspect | sed -n 's/^wasm_tools: //p')
if [ -n "$wasm_tools" ] && [ "$(wasm-tools --version 2>/dev/null || true)" != "wasm-tools $wasm_tools" ]; then
  if [ "$("$tools/bin/wasm-tools" --version 2>/dev/null || true)" != "wasm-tools $wasm_tools" ]; then
    echo "petal.sh: installing wasm-tools $wasm_tools into $tools" >&2
    cargo install --locked --quiet --version "=$wasm_tools" --root "$tools" wasm-tools
  fi
  PATH="$tools/bin:$PATH"
  export PATH
fi

exec "$tools/bin/petal" "$@"
