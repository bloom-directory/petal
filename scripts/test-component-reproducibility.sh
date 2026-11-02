#!/bin/sh
# Compare real components and SDK archives from different checkout directories.
# The caller owns this fresh evidence directory; no recursive cleanup runs.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=${1:?usage: test-component-reproducibility.sh FRESH_EMPTY_DIRECTORY}
[ -d "$scratch" ] && [ ! -L "$scratch" ] && [ -z "$(ls -A "$scratch")" ] || exit 1
scratch=$(CDPATH= cd -- "$scratch" && pwd)
cd "$root"
cargo build --locked --quiet -p bloom-petal-cli
petal=$(cargo metadata --locked --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')/debug/petal
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
  side_env=
  if [ "$side" = b ]; then
    printf 'unrelated development report\n' > "$copy/audit-notes.md"
    # A developer's own Cargo config, with build.rustflags as a string reached
    # through include, keeps its flags beside the builder's path remapping.
    # Its release profile, one set through CARGO_PROFILE_RELEASE_*, and
    # build.incremental must not change the components: the builder sets its
    # profile on the command line and CARGO_INCREMENTAL=0, which override them.
    mkdir "$copy/.cargo"
    printf 'include = ["team.toml"]\n' > "$copy/.cargo/config.toml"
    printf '[build]\nrustflags = "--cfg petal_reproducibility_probe"\nincremental = true\n[profile.release]\nopt-level = 1\n' > "$copy/.cargo/team.toml"
    side_env=CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
  fi
  # Packaging must rebuild current sources and discard stale active output.
  mkdir -p "$copy/petal/conformance"
  printf 'stale component\n' > "$copy/petal/conformance/obsolete.wasm"
  # Cargo reads config from the directory it runs in.
  # shellcheck disable=SC2086 # side_env is one assignment or empty.
  (cd "$copy" && env $side_env "$petal" package --root . --config fixtures/repro-petal-build.toml --out "$scratch/$side.petal.tar.gz") > "$scratch/$side-package.json"
  test ! -e "$copy/petal/conformance/obsolete.wasm"
done
if ! grep -rqF petal_reproducibility_probe "$scratch/b/target/petal-routes/target/wasm32-unknown-unknown/release/.fingerprint"/petal-route-*; then
  echo "side b's configured build.rustflags did not reach the route crates" >&2
  exit 1
fi
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
    if grep -aqF -- "$location" "$scratch/b/petal/conformance/$route.wasm"; then
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
# A release build reads every locked package from Cargo's own copy of its
# source. A Cargo config paths override, which the lockfile cannot show, must
# make it refuse; an identical copy of one registry dependency is enough.
copy=$scratch/a
dependency=$(cargo metadata --locked --format-version 1 --manifest-path "$copy/target/petal-routes/workspace/Cargo.toml" \
  | python3 -I -c 'import json, os, sys; print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] if (p["source"] or "").startswith("registry+")))')
cp -R "$dependency" "$scratch/overridden-dependency"
mkdir "$copy/.cargo"
printf 'paths = ["%s"]\n' "$scratch/overridden-dependency" > "$copy/.cargo/config.toml"
if (cd "$copy" && PETAL_REQUIRE_LOCKED_SOURCES=1 "$petal" build --root . --config fixtures/repro-petal-build.toml) > /dev/null 2> "$scratch/a-paths.err"; then
  echo 'a release build accepted a Cargo config paths override' >&2
  exit 1
fi
grep -q 'resolves from a local path, not the locked registry+' "$scratch/a-paths.err"
rm "$copy/.cargo/config.toml"
rmdir "$copy/.cargo"
# The release workflow packages what a Petal's own build command produced,
# which can hold compile-time values a rebuild would not see. After a route
# changes, --no-build still ships the built components; package rebuilds.
route=$copy/fixtures/conformance-route/files/static.txt.rs
sed 's/canonical petal fixture/edited after the build/' "$route" > "$route.edited"
mv "$route.edited" "$route"
# Bloom installs every component as a route, so --no-build refuses one that
# has no route controller instead of reviving a removed route.
printf 'stale component\n' > "$copy/petal/conformance/obsolete.wasm"
if (cd "$copy" && "$petal" package --no-build --root . --config fixtures/repro-petal-build.toml --out "$scratch/a-stale.petal.tar.gz") > /dev/null 2> "$scratch/a-stale.err"; then
  echo 'package --no-build accepted a component with no route controller' >&2
  exit 1
fi
grep -q 'obsolete.wasm has no route controller' "$scratch/a-stale.err"
test ! -e "$scratch/a-stale.petal.tar.gz"
rm "$copy/petal/conformance/obsolete.wasm"
(cd "$copy" && "$petal" package --no-build --root . --config fixtures/repro-petal-build.toml --out "$scratch/a-no-build.petal.tar.gz") > "$scratch/a-no-build-package.json"
cmp "$scratch/a.petal.tar.gz" "$scratch/a-no-build.petal.tar.gz"
(cd "$copy" && "$petal" package --root . --config fixtures/repro-petal-build.toml --out "$scratch/a-rebuilt.petal.tar.gz") > "$scratch/a-rebuilt-package.json"
if cmp -s "$scratch/a.petal.tar.gz" "$scratch/a-rebuilt.petal.tar.gz"; then
  echo 'package did not rebuild the edited route' >&2
  exit 1
fi
