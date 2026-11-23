#!/bin/sh
# Package the byte-sum Petal with the ordinary Petal CLI and extract the loop
# target from the packaged component with the experiment's extractor.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exp=$(CDPATH= cd -- "$here/.." && pwd)
repo=$(CDPATH= cd -- "$exp/../.." && pwd)
out="$here/out/byte-sum"
entry='petal/byte-sum/sum/[text].json.wasm'
case $(rustc --version) in 'rustc 1.97.0 '*) ;; *) echo 'needs rustc 1.97.0' >&2; exit 1 ;; esac
cargo build --quiet --locked --manifest-path "$repo/Cargo.toml" -p bloom-petal-cli
cargo build --quiet --release --locked --manifest-path "$exp/tools/wasm-evidence/Cargo.toml"
petal="$repo/target/debug/petal"
evidence="$exp/tools/wasm-evidence/target/release/petal-wasm-evidence"
root="$here/byte-sum-petal"
cargo test --quiet --manifest-path "$root/route/Cargo.toml"
rm -rf "$out"; mkdir -p "$out"
(cd "$root" && "$petal" package --root "$root" --out "$out/byte-sum.petal.tar.gz") > "$out/package.log" 2>&1
"$evidence" extract "$out/byte-sum.petal.tar.gz" "$entry" bloom_verify_byte_sum "$out/extract" > /dev/null
index=$(sed -n 's/^ *"function_index": \([0-9]*\),$/\1/p' "$out/extract/extraction.json")
wasm-tools print "$out/extract/target-module.wasm" \
  | awk -v f="(func (;$index;)" 'index($0, f) {p=1} p && /^  \(func / && !index($0, f) {exit} p' \
  > "$out/target-function.wat"
cat "$out/target-function.wat"
