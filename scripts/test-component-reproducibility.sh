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
# Cargo runs RUSTC_WRAPPER in place of rustc, with rustc and its arguments as
# the wrapper's own. Side b builds through this one, which records each
# compiler command, to show which flags reached which crate. Cargo documents
# that variable; where it keeps its build cache is its own business, and
# Cargo 1.100 moved it. The wrapper is a compiled program because macOS
# removes the library path rustc's linker needs from a system shell's
# environment.
recorder=$scratch/record-rustc
cat > "$recorder.rs" <<'EOF'
use std::io::Write;
use std::os::unix::process::CommandExt;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let rustc = args.next().expect("the compiler to run");
    let args: Vec<_> = args.collect();
    let mut line = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    line.push('\n');
    let log = std::env::var_os("PETAL_REPRO_RUSTC_LOG").expect("PETAL_REPRO_RUSTC_LOG");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .expect("record the compiler command");
    let error = std::process::Command::new(rustc).args(args).exec();
    panic!("run the compiler: {error}");
}
EOF
rustc --edition 2021 -o "$recorder" "$recorder.rs"
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
    # Nor must a rustc wrapper, as a compiler cache is.
    mkdir "$copy/.cargo"
    printf 'include = ["team.toml"]\n' > "$copy/.cargo/config.toml"
    printf '[build]\nrustflags = "--cfg petal_reproducibility_probe"\nincremental = true\n[profile.release]\nopt-level = 1\n' > "$copy/.cargo/team.toml"
    side_env=CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
  fi
  # Packaging must rebuild current sources and discard stale active output.
  mkdir -p "$copy/petal/conformance"
  printf 'stale component\n' > "$copy/petal/conformance/obsolete.wasm"
  # Cargo reads config from the directory it runs in.
  (
    cd "$copy"
    if [ "$side" = b ]; then
      export RUSTC_WRAPPER="$recorder" PETAL_REPRO_RUSTC_LOG="$scratch/b-rustc.log"
    fi
    # shellcheck disable=SC2086 # side_env is one assignment or empty.
    env $side_env "$petal" package --root . --config fixtures/repro-petal-build.toml --out "$scratch/$side.petal.tar.gz"
  ) > "$scratch/$side-package.json"
  test ! -e "$copy/petal/conformance/obsolete.wasm"
done
# Each recorded line is one compiler command. A route crate's must carry the
# flag from side b's Cargo config.
if ! grep -F -- '--crate-name petal_route_' "$scratch/b-rustc.log" |
  grep -qF -- '--cfg petal_reproducibility_probe'; then
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
# A release build refuses Cargo config in the checkout, which could move a
# package, wrap rustc or change flags without changing the lockfile. A paths
# override of an identical copy of one registry dependency is enough.
copy=$scratch/a
dependency=$(cargo metadata --locked --format-version 1 --manifest-path "$copy/target/petal-routes/workspace/Cargo.toml" \
  | python3 -I -c 'import json, os, sys; print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] if (p["source"] or "").startswith("registry+")))')
cp -R "$dependency" "$scratch/overridden-dependency"
override=$(printf 'paths = ["%s"]' "$scratch/overridden-dependency")
mkdir "$copy/.cargo"
printf '%s\n' "$override" > "$copy/.cargo/config.toml"
if (cd "$copy" && PETAL_REQUIRE_LOCKED_SOURCES=1 "$petal" build --root . --config fixtures/repro-petal-build.toml) > /dev/null 2> "$scratch/a-config.err"; then
  echo 'a release build accepted Cargo config in the checkout' >&2
  exit 1
fi
grep -q 'PETAL_REQUIRE_LOCKED_SOURCES=1 refuses Cargo config' "$scratch/a-config.err"
rm "$copy/.cargo/config.toml"
rmdir "$copy/.cargo"
# So is a rust-toolchain file that names a toolchain by path.
printf '[toolchain]\npath = "%s"\n' "$scratch/toolchain" > "$copy/rust-toolchain.toml"
if (cd "$copy" && PETAL_REQUIRE_LOCKED_SOURCES=1 "$petal" check --root . --config fixtures/repro-petal-build.toml) > /dev/null 2> "$scratch/a-toolchain.err"; then
  echo 'a release check accepted a rust-toolchain file naming a toolchain by path' >&2
  exit 1
fi
grep -q 'refuses a rust-toolchain file that names a toolchain by path' "$scratch/a-toolchain.err"
rm "$copy/rust-toolchain.toml"
# Cargo home's own config belongs to the machine, so the same override there
# passes that check; the build must still refuse the package it moves.
home=${CARGO_HOME:-$HOME/.cargo}
mkdir "$scratch/cargo-home"
for cached in registry git; do
  if [ -d "$home/$cached" ]; then
    ln -s "$home/$cached" "$scratch/cargo-home/$cached"
  fi
done
printf '%s\n' "$override" > "$scratch/cargo-home/config.toml"
if (cd "$copy" && CARGO_HOME="$scratch/cargo-home" PETAL_REQUIRE_LOCKED_SOURCES=1 "$petal" build --root . --config fixtures/repro-petal-build.toml) > /dev/null 2> "$scratch/a-paths.err"; then
  echo 'a release build accepted a package moved by a Cargo home paths override' >&2
  exit 1
fi
grep -q 'which the tested lockfile does not name' "$scratch/a-paths.err"
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
# That rebuild removed the components it replaced, which the first package
# recorded. Only the first package's copy of the planted component remains.
set -- "$copy/target/petal-routes/staging"/*
if [ "$#" -ne 1 ] || [ "$(ls -A "$1")" != obsolete.wasm ]; then
  echo 'builds left more under staging than the planted component' >&2
  ls -AR "$copy/target/petal-routes/staging" >&2
  exit 1
fi
