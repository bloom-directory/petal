#!/bin/bash
# Compile, audit and kernel-recheck one loops-io experiment.
# Usage: rocq/check.sh byte-sum|upper
# Needs the pinned toolchain on PATH (opam switch, see ../README.md) and
# WASMCERT pointing at a WasmCert-Coq checkout at 58a3d3f8 with
# theories/{opsem,binary_format_parser,memory_list}.vo built.
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exp=$1
# $exp.units lists the compilation units in order; the last prints assumptions.
units=$(cat "$here/$exp.units")
upstream=${WASMCERT:?set WASMCERT}/_build/default/theories
out="$here/../out/rocq/$exp"
mkdir -p "$out"
cd "$out"
cp "$here"/*.v . 2>/dev/null || true
cp "$here/../out/rocq/"*Bytes.v .
rm -f ./*.vo ./*.vos ./*.vok ./*.glob ./*.log
printf 'RUNNING\n' > status.txt
trap 'printf "FAILED\n" > status.txt' EXIT
printf 'stage\tseconds\n' > timings.tsv
shasum -a 256 $(for u in $units; do echo "$u.v"; done) > sources.sha256
{ rocq --version; git -C "$WASMCERT" rev-parse HEAD; uname -srm; } > toolchain.txt
now() { python3 -c 'import time; print(time.time())'; }
for unit in $units; do
  start=$(now)
  coqc -q -R "$upstream" Wasm -Q . '' "$unit.v" > "$unit.log" 2>&1 || { cat "$unit.log"; exit 1; }
  end=$(now)
  python3 -c "print('$unit\t%.3f' % ($end-$start))" >> timings.tsv
done
last=${units##* }
awk '/^[A-Za-z][A-Za-z0-9_.]*[[:space:]]*:/ {
  sub(/[[:space:]]*:.*/, ""); if ($0 != "Axioms") print
}' "$last.log" | LC_ALL=C sort -u > assumptions.txt
diff -u "$here/allowed-axioms.txt" assumptions.txt
start=$(now)
coqchk -silent -R "$upstream" Wasm -Q . '' "$last" > kernel-check.log 2>&1 || { tail -20 kernel-check.log; exit 1; }
end=$(now)
python3 -c "print('coqchk\t%.3f' % ($end-$start))" >> timings.tsv
cat timings.tsv
cat "$last.log"
printf 'PASS\n' > status.txt
trap - EXIT
