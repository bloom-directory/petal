#!/bin/sh
# Compare real components and SDK archives from different checkout directories.
# The caller owns this fresh evidence directory; no recursive cleanup runs.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=${1:?usage: test-component-reproducibility.sh FRESH_EMPTY_DIRECTORY}
[ -d "$scratch" ] && [ ! -L "$scratch" ] && [ -z "$(ls -A "$scratch")" ] || exit 1
scratch=$(CDPATH= cd -- "$scratch" && pwd)
cd "$root"
for side in a b; do
  copy=$scratch/$side
  mkdir "$copy"
  git archive HEAD | tar -x -C "$copy"
  # Runtime identity covers executable output, its security declarations and
  # the host's two runtime documents, independently of SDK development files.
  cat > "$copy/petal.toml" <<'EOF'
schema = "bloom.petal.package.v1"
name = "conformance"
[caps]
allowed = ["bloom:sign"]
[sign]
allowed_intents = ["conformance.sign"]
EOF
  printf '# Structural conformance fixture\nNo signing authorization is implied.\n' > "$copy/README.md"
  printf '# Fixture guidance\nUse only for component and package checks.\n' > "$copy/AGENTS.md"
  if [ "$side" = b ]; then printf 'unrelated development report\n' > "$copy/audit-notes.md"; fi
  # Packaging must rebuild current sources and discard stale active output.
  mkdir -p "$copy/petal/conformance"
  printf 'stale component\n' > "$copy/petal/conformance/obsolete.wasm"
  cargo run --locked --quiet -p bloom-petal-cli -- package --root "$copy" --config fixtures/repro-petal-build.toml --out "$scratch/$side.petal.tar.gz" > "$scratch/$side-package.json"
  test ! -e "$copy/petal/conformance/obsolete.wasm"
done
for route in sign static.txt; do
  if ! cmp "$scratch/a/petal/conformance/$route.wasm" "$scratch/b/petal/conformance/$route.wasm"; then
    for side in a b; do
      wasm-tools print "$scratch/$side/petal/conformance/$route.wasm" > "$scratch/reproducibility-$side.wat"
    done
    diff -u "$scratch/reproducibility-a.wat" "$scratch/reproducibility-b.wat" | head -80 || true
    printf 'Component mismatch; diagnostic evidence retained at %s\n' "$scratch" >&2
    exit 1
  fi
done
# Components name no machine-specific location: not the checkout, the Rust
# toolchain, which rust-src exposes, or the home directory holding them.
sysroot=$(rustc --print sysroot)
for route in sign static.txt; do
  for location in "$scratch" "$sysroot" "${HOME:-/}"; do
    [ "$location" = / ] && continue
    if grep -aqF -- "$location" "$scratch/a/petal/conformance/$route.wasm"; then
      printf '%s embeds %s\n' "$route.wasm" "$location" >&2
      exit 1
    fi
  done
done
cmp "$scratch/a.petal.tar.gz" "$scratch/b.petal.tar.gz"
python3 - "$scratch" <<'PY'
import hashlib, json, sys
from pathlib import Path
root = Path(sys.argv[1])
a, b = [json.loads((root / f'{side}-package.json').read_text()) for side in ['a', 'b']]
assert a['package_hash'] == b['package_hash']
assert a['sha256'] == b['sha256']
result = {'schema': 'bloom.petal.reproducibility.v1', 'components': {
    path.name: hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted((root / 'a/petal/conformance').glob('*.wasm'))},
    'sdk_package_hash': a['package_hash'], 'archive_sha256': a['sha256'],
    'machine_composition': 'not_run', 'authorization': 'not_run'}
(root / 'reproducibility-result.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
PY
# The release workflow packages what a Petal's own build command produced,
# which can hold compile-time values a rebuild would not see. After a route
# changes, --no-build still ships the built components; package rebuilds.
copy=$scratch/a
route=$copy/fixtures/conformance-route/files/static.txt.rs
sed 's/canonical petal fixture/edited after the build/' "$route" > "$route.edited"
mv "$route.edited" "$route"
cargo run --locked --quiet -p bloom-petal-cli -- package --no-build --root "$copy" --config fixtures/repro-petal-build.toml --out "$scratch/a-no-build.petal.tar.gz" > "$scratch/a-no-build-package.json"
cmp "$scratch/a.petal.tar.gz" "$scratch/a-no-build.petal.tar.gz"
cargo run --locked --quiet -p bloom-petal-cli -- package --root "$copy" --config fixtures/repro-petal-build.toml --out "$scratch/a-rebuilt.petal.tar.gz" > "$scratch/a-rebuilt-package.json"
if cmp -s "$scratch/a.petal.tar.gz" "$scratch/a-rebuilt.petal.tar.gz"; then
  echo 'package did not rebuild the edited route' >&2
  exit 1
fi
