#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export REAL_CARGO
REAL_CARGO=$(command -v cargo)
export INSTALL_LOG="$tmp/install.log"
mkdir -p "$tmp/pinned tooling/scripts" "$tmp/pinned tooling/src" "$tmp/bin" "$tmp/caller"
cp "$root/scripts/install-tools.sh" "$tmp/pinned tooling/scripts/"
touch "$tmp/pinned tooling/src/lib.rs"
cat > "$tmp/manifest.toml" <<'MANIFEST'
[package]
name = "bloom-petal-sdk"
version = "0.1.0"
edition = "2021"
[workspace.metadata.tools]
wasm-tools = "=1.2.3"
[workspace.dependencies]
wit-bindgen = "=0.4.5"
[dependencies]
wit-bindgen.workspace = true
MANIFEST
# A caller's manifest must never determine the pinned tooling versions.
echo 'not valid TOML' > "$tmp/caller/Cargo.toml"
cat > "$tmp/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == metadata ]]; then
  exec "$REAL_CARGO" "$@"
fi
printf '%s\n' "$*" >> "$INSTALL_LOG"
package=$2
version=${4#=}
if [[ "$package" == "${WRONG_PACKAGE:-}" ]]; then
  version=9.9.9
fi
executable=$package
[[ "$package" != wit-bindgen-cli ]] || executable=wit-bindgen
binary="$(dirname -- "$0")/$executable"
printf '#!/bin/sh\necho "%s %s"\n' "$package" "$version" > "$binary"
chmod +x "$binary"
CARGO
chmod +x "$tmp/bin/cargo"
export PATH="$tmp/bin:$PATH"
cd "$tmp/caller"
installer="$tmp/pinned tooling/scripts/install-tools.sh"
manifest="$tmp/pinned tooling/Cargo.toml"

# Both tools use the installer's manifest and exact, locked installs.
cp "$tmp/manifest.toml" "$manifest"
"$installer"
printf '%s\n' \
  'install wasm-tools --version =1.2.3 --locked' \
  'install wit-bindgen-cli --version =0.4.5 --locked' > "$tmp/expected"
diff -u "$tmp/expected" "$INSTALL_LOG"

# The wasm-only mode does not need the SDK binding dependency.
rm "$INSTALL_LOG"
sed '/wit-bindgen.workspace = true/d' "$tmp/manifest.toml" > "$manifest"
"$installer" --wasm-tools-only
echo 'install wasm-tools --version =1.2.3 --locked' > "$tmp/expected"
diff -u "$tmp/expected" "$INSTALL_LOG"

# Either non-exact pin must fail before any installation.
for version in 1.2.3 0.4.5; do
  rm -f "$INSTALL_LOG"
  sed "s/=$version/$version/" "$tmp/manifest.toml" > "$manifest"
  if "$installer" > "$tmp/output" 2>&1; then
    echo "installer accepted an unpinned version: $version" >&2
    exit 1
  fi
  grep -q 'expected an exact' "$tmp/output"
  test ! -e "$INSTALL_LOG"
done

# Verification fails if PATH selects the wrong version.
cp "$tmp/manifest.toml" "$manifest"
for package in wasm-tools wit-bindgen-cli; do
  if WRONG_PACKAGE="$package" "$installer" > "$tmp/output" 2>&1; then
    echo "installer accepted the wrong executable version for $package" >&2
    exit 1
  fi
  if [[ "$package" == wasm-tools ]]; then
    expected="expected 'wasm-tools 1.2.3' from 'wasm-tools --version'"
  else
    expected="expected 'wit-bindgen-cli 0.4.5' from 'wit-bindgen --version'"
  fi
  grep -Fq "$expected" "$tmp/output"
  grep -Fq "got '$package 9.9.9'" "$tmp/output"
done
echo 'Installer tests passed'
