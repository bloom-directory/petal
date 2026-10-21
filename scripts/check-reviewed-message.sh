#!/bin/sh
# Read public producer fixtures through a supplied actual Broker checkout.
# All build/test edits stay in a fresh retained scratch directory.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
broker=${1:?usage: check-reviewed-message.sh BROKER_CHECKOUT SIGNER_ADDRESS [PUBLIC_FIXTURE_JSON]}
signer=${2:?supply the selected signer address; this preview grants no authority}
fixture=${3:-}
broker=$(CDPATH= cd -- "$broker" && pwd)
revision=$(git -C "$broker" rev-parse HEAD)
if ! git -C "$broker" diff --quiet HEAD -- Cargo.toml Cargo.lock crates; then
  printf 'Broker source differs from HEAD; commit the tested source before previewing a recorded revision.\n' >&2
  exit 1
fi
sdk_revision=$(git -C "$root" rev-parse HEAD)
producer=supplied_fixture
scratch=$(mktemp -d)
printf 'Decoder test evidence retained at %s\n' "$scratch" >&2
if [ -z "$fixture" ]; then
  producer=generated_sdk_example
  fixture=$scratch/producer.json
  (cd "$root" && cargo run --locked --quiet -p bloom-petal-sdk --features evm-messages --example reviewed_message) > "$fixture"
fi
fixture=$(cd "$(dirname -- "$fixture")" && pwd)/$(basename -- "$fixture")
mkdir "$scratch/broker"
git -C "$broker" archive HEAD | tar -x -C "$scratch/broker"
cp "$root/scripts/broker-message-fixture.rs" "$scratch/broker/crates/bloom-broker/src/sdk_message_fixture.rs"
cat >> "$scratch/broker/crates/bloom-broker/src/message_review.rs" <<'EOF'

#[cfg(test)]
#[path = "sdk_message_fixture.rs"]
mod sdk_message_fixture;
EOF
export PETAL_PUBLIC_FIXTURE=$fixture PETAL_PREVIEW_SIGNER=$signer
export PETAL_BROKER_READINGS=$scratch/readings.json
if ! cargo test --locked --manifest-path "$scratch/broker/Cargo.toml" -p bloom-broker --lib \
  message_review::sdk_message_fixture::producer_bytes_match_actual_broker_reading -- --exact \
  > "$scratch/decoder-test.log" 2>&1; then
  printf 'Broker decoder check failed; inspect %s/decoder-test.log\n' "$scratch" >&2
  exit 1
fi
python3 - "$scratch/readings.json" "$revision" "$sdk_revision" "$fixture" "$producer" <<'PY'
import hashlib, json, sys
from pathlib import Path
readings = json.loads(Path(sys.argv[1]).read_text())
print(json.dumps({'broker_revision': sys.argv[2], 'sdk_revision': sys.argv[3],
    'fixture_sha256': hashlib.sha256(Path(sys.argv[4]).read_bytes()).hexdigest(),
    'producer': sys.argv[5], 'decoded': True,
    'authorized_in_test': False, 'review': readings,
    'limits': 'Actual Broker message reader; no Machine installation, policy, owner approval, signature or settlement.'}, indent=2))
PY
