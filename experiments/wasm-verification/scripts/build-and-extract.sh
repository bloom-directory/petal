#!/bin/sh
# Milestone 1: build and package the correct and faulty split-even Petals with
# the ordinary Petal CLI, then extract, decode, harness and concretely test the
# verification target from each packaged archive.
#
# Usage: scripts/build-and-extract.sh
# Requires wasm-tools 1.249.0 (the workspace pin) first on PATH.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
repo=$(CDPATH= cd -- "$here/../.." && pwd)
out="$here/out"
entry='petal/split-even/split/[total].json.wasm'
export_name=bloom_verify_split_even

pin=$(sed -n 's/^wasm-tools = "=\(.*\)"$/\1/p' "$repo/Cargo.toml")
have=$(wasm-tools --version | cut -d' ' -f2)
if [ "$have" != "$pin" ]; then
  echo "wasm-tools $have on PATH; the workspace pins $pin" >&2
  exit 1
fi

case $(rustc --version) in
  'rustc 1.97.0 '*) ;;
  *) echo 'This experiment pins rustc 1.97.0; select it before building.' >&2; exit 1 ;;
esac
mkdir -p "$out"
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "repo_commit: $(git -C "$repo" rev-parse HEAD)"
  echo "repo_dirty_paths: $(git -C "$repo" status --porcelain | wc -l | tr -d ' ')"
  rustc -Vv
  cargo -V
  wasm-tools --version
  uname -srm
} > "$out/toolchain.txt"

cargo build --quiet --locked --manifest-path "$repo/Cargo.toml" -p bloom-petal-cli
petal="$repo/target/debug/petal"
cargo build --quiet --release --locked --manifest-path "$here/tools/wasm-evidence/Cargo.toml"
evidence="$here/tools/wasm-evidence/target/release/petal-wasm-evidence"

cargo test --quiet --locked --manifest-path "$here/split-even-petal/route/Cargo.toml"

# The faulty variant is the same Petal with one committed source file swapped.
# It sits two directories deeper, so its SDK path gains two `../`.
rm -rf "$out/faulty/petal-src"
mkdir -p "$out/faulty/petal-src"
(cd "$here/split-even-petal" && tar cf - --exclude ./petal --exclude ./route/target --exclude ./target .) \
  | (cd "$out/faulty/petal-src" && tar xf -)
cp "$here/variants/faulty/split.rs" "$out/faulty/petal-src/route/src/split.rs"
sed -i.orig 's|path = "../../../../crates/petal-sdk"|path = "../../../../../../crates/petal-sdk"|' \
  "$out/faulty/petal-src/route/Cargo.toml"
rm "$out/faulty/petal-src/route/Cargo.toml.orig"
grep -q '"../../../../../../crates/petal-sdk"' "$out/faulty/petal-src/route/Cargo.toml"

for variant in correct faulty; do
  case $variant in
    correct) root="$here/split-even-petal" ;;
    faulty) root="$out/faulty/petal-src" ;;
  esac
  dir="$out/$variant"
  rm -rf "$dir/extract" "$dir/harness" "$dir/split-even.petal.tar.gz"
  mkdir -p "$dir"
  start=$(date +%s)
  (cd "$root" && "$petal" package --root "$root" --out "$dir/split-even.petal.tar.gz") > "$dir/package.log" 2>&1
  echo "package_seconds: $(( $(date +%s) - start ))" > "$dir/timings.txt"
  "$evidence" extract "$dir/split-even.petal.tar.gz" "$entry" "$export_name" "$dir/extract" > /dev/null
  "$evidence" harness "$dir/extract/target-module.wasm" "$export_name" "$dir/harness" > /dev/null
  "$evidence" run "$dir/extract/target-module.wasm" "$export_name" > "$dir/concrete-run.json"
  index=$(sed -n 's/^ *"function_index": \([0-9]*\),$/\1/p' "$dir/extract/extraction.json")
  wasm-tools print "$dir/extract/target-module.wasm" \
    | awk -v f="(func (;$index;)" 'index($0, f) {p=1} p && /^  \(func / && !index($0, f) {exit} p' \
    > "$dir/target-function.wat"
  wasm-tools print "$dir/harness/sunbeam-harness.wasm" > "$dir/harness/sunbeam-harness.wat"
  echo "$variant: archive $(shasum -a 256 "$dir/split-even.petal.tar.gz" | cut -d' ' -f1)"
done
