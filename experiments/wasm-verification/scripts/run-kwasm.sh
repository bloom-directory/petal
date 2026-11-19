#!/bin/sh
# Milestone 1, second backend: KWasm (runtimeverification/wasm-semantics).
#
# Usage: scripts/run-kwasm.sh   (after scripts/build-and-extract.sh)
#
# Runs in the pinned K image (linux/amd64; emulated on Apple silicon). The
# container gets the KWasm checkout read-only and out/ read-write.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$here/out"
kwasm_src=${KWASM_SRC:?set KWASM_SRC to a wasm-semantics checkout at v0.1.157}
image=runtimeverificationinc/kframework-k:ubuntu-jammy-7.1.329@sha256:4864e4dcef177614d9b4fc3c787932ba1d5ca579366af609f0707e6e5d547b4b
name=petal-m1-kwasm

test "$(git -C "$kwasm_src" rev-parse HEAD)" = 212271bd434bd402e27c42f6416854342733386d
git -C "$kwasm_src" diff --quiet HEAD --
mkdir -p "$out/kwasm"

if ! docker inspect "$name" > /dev/null 2>&1; then
  docker run -d --name "$name" --platform linux/amd64 \
    -v "$kwasm_src:/src/kwasm:ro" -v "$out:/work" "$image" sleep infinity > /dev/null
  docker cp "$here/kwasm/requirements.lock" "$name:/opt/requirements.lock"
  docker exec "$name" sh -c '
    set -eu
    cp -r /src/kwasm /opt/kwasm
    python3 -m venv /opt/venv
    /opt/venv/bin/pip install -q -r /opt/requirements.lock
    /opt/venv/bin/pip install -q --no-deps /opt/kwasm/pykwasm
    cd /opt && /opt/venv/bin/kwasm-kdist build wasm-semantics.kwasm-lemmas'
fi
# Refuse a stale container or a different checkout/output mount.
test "$(docker inspect "$name" --format '{{.Image}}')" = "$(docker image inspect "$image" --format '{{.Id}}')"
test "$(docker inspect "$name" --format '{{range .Mounts}}{{if eq .Destination "/work"}}{{.Source}}{{end}}{{end}}')" = "$out"
test "$(docker inspect "$name" --format '{{.State.Running}}')" = true
docker exec "$name" sh -ec '
  test "$(git -C /opt/kwasm rev-parse HEAD)" = 212271bd434bd402e27c42f6416854342733386d
  git -C /opt/kwasm diff --quiet HEAD --
'
docker exec "$name" /opt/venv/bin/python -I -c 'import importlib.metadata as m; assert m.version("kframework") == "7.1.329"; assert m.version("pykwasm") == "0.1.157"'
docker exec "$name" /opt/venv/bin/pip freeze > "$out/kwasm/pip-freeze.txt"
python3 - "$here/kwasm/requirements.lock" "$out/kwasm/pip-freeze.txt" <<'PYLOCK'
import sys
from pathlib import Path
expected = set(Path(sys.argv[1]).read_text().splitlines())
actual = {line for line in Path(sys.argv[2]).read_text().splitlines() if not line.startswith('pykwasm ')}
if expected != actual:
    raise SystemExit('KWasm Python dependencies differ from the recorded lock')
PYLOCK
docker cp "$here/kwasm/gen_claims.py" "$name:/opt/gen_claims.py"
docker cp "$here/kwasm/split-even-lemmas.md" "$name:/opt/split-even-lemmas.md"
mkdir -p "$out/kwasm"
{
  echo "image: $image ($(docker image inspect "$image" --format '{{.Id}}'))"
  echo "kwasm: $(git -C "$kwasm_src" describe --tags) $(git -C "$kwasm_src" rev-parse HEAD)"
  docker exec "$name" sh -c 'kompile --version | head -1; z3 --version; /opt/venv/bin/pip freeze | grep -E "^(kframework|pykwasm)"'
} > "$out/kwasm/tools.txt"

# Whole-module decoding with KWasm's own binary parser (expected to fail on
# the shipped module; the outcome is recorded, not assumed).
for variant in correct faulty; do
  docker exec "$name" sh -c "cd /work && /opt/venv/bin/wasm2kast $variant/extract/target-module.wasm \
    > /dev/null 2> kwasm/$variant-wasm2kast.log; echo \"exit \$?\" >> kwasm/$variant-wasm2kast.log"
done

printf 'variant\tclaim\tverdict\texit\tseconds\n' > "$out/kwasm/summary.tsv"
for variant in correct faulty; do
  docker exec "$name" sh -c "
    K=\$(ls -d /root/.cache/kdist-*/wasm-semantics/kwasm-lemmas)
    cd /work && /opt/venv/bin/python -I /opt/gen_claims.py $variant/extract/extraction.json \
      $variant/extract/target-module.wasm \$K kwasm/$variant > kwasm/$variant-decoded.json"
  for claim in returns props props-lemma frame-below frame-above exact-lemma; do
    spec="$variant-$claim-spec.k"
    start=$(date +%s)
    set +e
    docker exec "$name" sh -c "
      I=\$(dirname \$(find /root/.cache/kdist-*/wasm-semantics/source -name kwasm-lemmas.md))
      cd /work/kwasm && /opt/venv/bin/kwasm prove -I \$I -I /opt $spec kwasm-lemmas > $variant-$claim-prove.log 2>&1"
    code=$?
    set -e
    seconds=$(( $(date +%s) - start ))
    # A successful run with the custom rewrite is conditional, never proof success.
    verdict=$(python3 "$here/scripts/verdicts.py" kwasm "$out/kwasm/$variant-$claim-prove.log" "$claim" "$code")
    printf '%s\t%s\t%s\t%s\t%s\n' "$variant" "$claim" "$verdict" "$code" "$seconds" >> "$out/kwasm/summary.tsv"
    echo "$variant $claim: $verdict ($code, ${seconds}s)"
  done
done
