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
sdk_revision=$(git -C "$root" rev-parse HEAD)
if [ -n "$(git -C "$root" status --porcelain --untracked-files=all -- Cargo.toml Cargo.lock crates fixtures scripts src wit templates)" ]; then
  printf 'Commit the SDK component/check sources before recording their revision.\n' >&2
  exit 1
fi
scratch=$(mktemp -d)
printf 'Component check evidence retained at %s\n' "$scratch" >&2
cd "$root"
cargo run --locked --quiet -p bloom-petal-cli -- build --config fixtures/petal-build.toml > "$scratch/build.log"
cargo run --locked --quiet -p bloom-petal-cli -- inspect > "$scratch/sdk-contract.txt"
for side in a b; do
  package=$scratch/package-$side
  mkdir -p "$package/petal/conformance"
  cp -R "$root/fixtures/components/." "$package/petal/conformance/"
  cat > "$package/petal.toml" <<'EOF'
schema = "bloom.petal.package.v1"
name = "conformance"
[caps]
allowed = ["bloom:sign", "bloom:tx.outbox", "bloom:vfs.read"]
[sign]
allowed_intents = ["conformance.sign", "conformance.personal"]
EOF
  printf '# Public component fixture\nNo authorization or signature.\n' > "$package/README.md"
  printf '# Fixture\nUse only for bounded component checks.\n' > "$package/AGENTS.md"
done
mkdir "$scratch/machine"
git -C "$machine" archive HEAD | tar -x -C "$scratch/machine"
cp "$root/scripts/machine-message-fixture.rs" "$scratch/machine/crates/bloom-petals/tests/sdk_message_fixture.rs"
export PETAL_COMPONENT_EVIDENCE=$scratch PETAL_FIXTURE_SIGNER=$signer
if ! cargo test --locked --manifest-path "$scratch/machine/Cargo.toml" -p bloom-petals \
  --target-dir "$root/target/host-check" --test sdk_message_fixture -- --nocapture > "$scratch/machine-test.log" 2>&1; then
  printf 'Machine component check failed; inspect %s/machine-test.log\n' "$scratch" >&2
  exit 1
fi
[ "$(git -C "$root" rev-parse HEAD)" = "$sdk_revision" ] || { printf 'SDK revision changed during the check.\n' >&2; exit 1; }
sh "$root/scripts/check-reviewed-message.sh" "$broker" "$signer" "$scratch/producer.json" > "$scratch/broker-review.json"
python3 - "$scratch" "$revision" "$sdk_revision" <<'PY'
import hashlib, json, sys
from pathlib import Path
root = Path(sys.argv[1])
decoder = json.loads((root/'broker-review.json').read_text())
assert decoder['sdk_revision'] == sys.argv[3] and not decoder['sdk_worktree_dirty']
sdk_contract = dict(line.split(': ', 1) for line in (root/'sdk-contract.txt').read_text().splitlines())
assert set(sdk_contract) == {'contract', 'wit_digest'}
result = {'machine_revision':sys.argv[2], 'sdk_revision':sys.argv[3], 'built':True,
    'sdk_contract':sdk_contract,
    'structurally_valid':True, 'compiled_capability_refusal':True,
    'compiled_capability_reason':(root/'capability-refusal.txt').read_text(),
    'wasm_forwarded':True, 'account':1, 'retry_identity_preserved':True,
    'transaction_staging_forwarded':True, 'transaction_confirmed':False,
    'machine_composition_reproduced':True, 'machine_archive_reproduced':True,
    'archive_sha256':hashlib.sha256((root/'composed.petal.tar').read_bytes()).hexdigest(),
    'composition':json.loads((root/'composition.json').read_text()),
    'machine_test_adapter_sha256':hashlib.sha256((root/'machine/crates/bloom-petals/tests/sdk_message_fixture.rs').read_bytes()).hexdigest(),
    'decoder':decoder,
    'authorized_in_test':False,
    'limits':'Test-only Machine/Broker adapters; captured host, synthetic account projection, no daemon authorization, custody signature, chain submission or settlement.'}
(root/'component-result.json').write_text(json.dumps(result, indent=2)+'\n')
print(json.dumps(result, indent=2))
PY
