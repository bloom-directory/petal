#!/bin/sh
# Run the petal CLI of the SDK release this Petal pins.
#
# route/Cargo.toml names the SDK tag, and route/Cargo.lock records its commit.
# On first use this installs the petal CLI from that commit, with the
# wasm-tools release it builds with, under ~/.cache/bloom-petal. So the SDK
# tag is the only version to change. PETAL_BIN names another binary, as the
# release workflow does.
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
case "$source" in
  git+https://github.com/bloom-directory/petal\?*\#*)
    commit=${source##*#}
    ;;
  "")
    # A Cargo config [patch] puts the SDK at a local checkout: use the petal
    # CLI on PATH, built from that checkout.
    exec petal "$@"
    ;;
  *)
    echo "petal.sh: $lock takes bloom-petal-sdk from $source; pin a release tag of https://github.com/bloom-directory/petal" >&2
    exit 1
    ;;
esac

tools=${PETAL_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/bloom-petal}/$commit
if [ ! -x "$tools/bin/petal" ]; then
  echo "petal.sh: installing the petal CLI at $commit into $tools" >&2
  cargo install --locked --quiet --git https://github.com/bloom-directory/petal --rev "$commit" --root "$tools" bloom-petal-cli
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
