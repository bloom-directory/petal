#!/usr/bin/env bash
set -euo pipefail

wasm_tools_only=false
case "${1:-}" in
  --wasm-tools-only) wasm_tools_only=true; shift ;;
  -h|--help)
    echo 'Usage: install-tools.sh [--wasm-tools-only]'
    exit 0
    ;;
esac
if [[ $# -ne 0 ]]; then
  echo 'Usage: install-tools.sh [--wasm-tools-only]' >&2
  exit 1
fi

root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

# Let Cargo parse the manifest without downloading or building dependencies.
metadata=$(cargo metadata --offline --no-deps --format-version 1 --manifest-path "$root/Cargo.toml")
packages=(wasm-tools)
executables=(wasm-tools)
versions=("$(jq -er '.metadata.tools["wasm-tools"] | strings' <<<"$metadata")")
if [[ "$wasm_tools_only" == false ]]; then
  packages+=(wit-bindgen-cli)
  executables+=(wit-bindgen)
  versions+=("$(jq -er '
    [.packages[] | select(.name == "bloom-petal-sdk")
      | .dependencies[] | select(.name == "wit-bindgen") | .req]
    | if length == 1 then .[0] | strings else error("expected one SDK wit-bindgen dependency") end
  ' <<<"$metadata")")
fi

# Validate all pins before installing anything.
for version in "${versions[@]}"; do
  if [[ ! "$version" =~ ^=[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "install-tools: expected an exact =major.minor.patch tool version, got '$version'" >&2
    exit 1
  fi
done

for i in "${!packages[@]}"; do
  package=${packages[$i]}
  executable=${executables[$i]}
  version=${versions[$i]#=}
  cargo install "$package" --version "=$version" --locked
  reported=$("$executable" --version)
  expected="$package $version"
  if [[ "$reported" != "$expected" ]]; then
    echo "install-tools: expected '$expected' from '$executable --version' on PATH, got '$reported'; put Cargo's installation bin directory first on PATH" >&2
    exit 1
  fi
  echo "Verified $reported"
done
