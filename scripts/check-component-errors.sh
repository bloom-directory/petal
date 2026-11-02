#!/bin/sh
# Diagnostic adapter: real SDK component and Machine VM, synthetic public errors.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
machine=${1:?usage: check-component-errors.sh MACHINE_CHECKOUT}
machine=$(CDPATH= cd -- "$machine" && pwd)
for checkout in "$root" "$machine"; do
  [ -z "$(git -C "$checkout" status --porcelain --untracked-files=all)" ] || {
    printf 'Commit check sources before recording revisions: %s\n' "$checkout" >&2
    exit 1
  }
done
sdk_revision=$(git -C "$root" rev-parse HEAD)
machine_revision=$(git -C "$machine" rev-parse HEAD)
scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/petal-errors.XXXXXX")
printf 'Component error evidence retained at %s\n' "$scratch" >&2
cd "$root"
cargo run --locked --quiet -p bloom-petal-cli -- build --config fixtures/petal-build.toml > "$scratch/build.log"
mkdir "$scratch/machine"
git -C "$machine" archive HEAD | tar -x -C "$scratch/machine"
cp scripts/machine-error-fixture.rs "$scratch/machine/crates/bloom-petals/tests/sdk_error_fixture.rs"
export PETAL_ERROR_COMPONENT="$root/fixtures/components/sign.wasm"
export PETAL_ERROR_OBSERVATIONS="$scratch/observed.json"
if ! cargo test --locked --manifest-path "$scratch/machine/Cargo.toml" -p bloom-petals \
  --target-dir "${PETAL_HOST_TEST_TARGET:-$root/target/host-check}" \
  --test sdk_error_fixture > "$scratch/machine-test.log" 2>&1; then
  printf 'Compiled error check failed; inspect %s/machine-test.log\n' "$scratch" >&2
  exit 1
fi
python3 - "$sdk_revision" "$machine_revision" "$scratch" <<'PY'
import hashlib, json, os, pathlib, sys
result = {'sdk_revision':sys.argv[1], 'machine_revision':sys.argv[2],
    'component_sha256':hashlib.sha256(pathlib.Path(os.environ['PETAL_ERROR_COMPONENT']).read_bytes()).hexdigest(),
    'feature':'host-error-detail', 'compiled_error_forwarding':True,
    'observations':json.loads(pathlib.Path(os.environ['PETAL_ERROR_OBSERVATIONS']).read_text()),
    'authorized_in_test':False,
    'limits':'Actual compiled component and Machine VM; synthetic host errors, no Broker, Signer, credentials or funds.'}
text = json.dumps(result, indent=2)+'\n'
pathlib.Path(sys.argv[3], 'error-result.json').write_text(text)
print(text, end='')
PY
