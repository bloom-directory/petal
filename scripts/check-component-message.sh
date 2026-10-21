#!/bin/sh
# Build a real SDK constructor component, validate/compose with Machine, then
# independently reconstruct its captured bytes through Broker. No authority.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
machine=${1:?usage: check-component-message.sh MACHINE_CHECKOUT BROKER_CHECKOUT SIGNER_ADDRESS}
broker=${2:?supply actual Broker source}
signer=${3:?supply selected signer address for public review}
machine=$(CDPATH= cd -- "$machine" && pwd)
if ! git -C "$machine" diff --quiet HEAD -- Cargo.toml Cargo.lock crates; then
  printf 'Commit the Machine source before recording its revision.\n' >&2
  exit 1
fi
revision=$(git -C "$machine" rev-parse HEAD)
scratch=$(mktemp -d)
printf 'Component check evidence retained at %s\n' "$scratch" >&2
cd "$root"
cargo run --locked --quiet -p bloom-petal-cli -- build --config fixtures/petal-build.toml > "$scratch/build.log"
for side in a b; do
  package=$scratch/package-$side
  mkdir -p "$package/petal/conformance"
  cp -R "$root/fixtures/components/." "$package/petal/conformance/"
  cat > "$package/petal.toml" <<'EOF'
schema = "bloom.petal.package.v1"
name = "conformance"
[caps]
allowed = ["bloom:sign"]
[sign]
allowed_intents = ["conformance.sign", "conformance.personal"]
EOF
  printf '# Public component fixture\nNo authorization or signature.\n' > "$package/README.md"
  printf '# Fixture\nUse only for bounded component checks.\n' > "$package/AGENTS.md"
done
mkdir "$scratch/machine"
git -C "$machine" archive HEAD | tar -x -C "$scratch/machine"
cp "$root/scripts/machine-message-fixture.rs" "$scratch/machine/crates/bloom-petals/tests/sdk_message_fixture.rs"
export PETAL_COMPONENT_EVIDENCE=$scratch
if ! cargo test --locked --manifest-path "$scratch/machine/Cargo.toml" -p bloom-petals \
  --test sdk_message_fixture -- --nocapture > "$scratch/machine-test.log" 2>&1; then
  printf 'Machine component check failed; inspect %s/machine-test.log\n' "$scratch" >&2
  exit 1
fi
sh "$root/scripts/check-reviewed-message.sh" "$broker" "$signer" "$scratch/producer.json" > "$scratch/broker-review.json"
python3 - "$scratch" "$revision" <<'PY'
import hashlib, json, sys
from pathlib import Path
root = Path(sys.argv[1])
result = {'machine_revision':sys.argv[2], 'built':True,
    'structurally_valid':True, 'compiled_capability_refusal':True,
    'wasm_forwarded':True, 'account':1, 'retry_identity_preserved':True,
    'machine_composition_reproduced':True, 'machine_archive_reproduced':True,
    'archive_sha256':hashlib.sha256((root/'composed.petal.tar').read_bytes()).hexdigest(),
    'composition':json.loads((root/'composition.json').read_text()),
    'decoder':json.loads((root/'broker-review.json').read_text()),
    'authorized_in_test':False,
    'limits':'Test-only Machine/Broker adapters; captured host, synthetic account projection, no daemon authorization, custody signature, chain submission or settlement.'}
(root/'component-result.json').write_text(json.dumps(result, indent=2)+'\n')
print(json.dumps(result, indent=2))
PY
