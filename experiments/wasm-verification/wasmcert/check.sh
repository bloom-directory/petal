#!/bin/bash
set -euo pipefail
cd /fixture/out/wasmcert
upstream=/home/rocq/petal-wasmcert/_build/default/theories
cp /fixture/wasmcert/*.v .
rm -f ./*.vo ./*.vos ./*.vok ./*.glob ./*.log
printf 'RUNNING\n' > status.txt
trap 'printf "FAILED\n" > status.txt' EXIT
printf 'stage\tseconds\n' > timings.tsv
sha256sum ./*.v > sources.sha256
opam list --installed --columns=name,version > toolchain.txt
opam exec -- rocq --version >> toolchain.txt
git -C /home/rocq/petal-wasmcert rev-parse HEAD >> toolchain.txt
for unit in ArtifactBytes Artifact Arithmetic MemoryFacts Execution Negative Assumptions; do
  start=$(date +%s%N)
  opam exec -- coqc -q -R "$upstream" Wasm -Q . '' "$unit.v" > "$unit.log" 2>&1
  end=$(date +%s%N)
  awk -v stage="$unit" -v delta="$((end-start))" \
    'BEGIN { printf "%s\t%.3f\n", stage, delta/1000000000 }' >> timings.tsv
done
awk '/^[A-Za-z][A-Za-z0-9_.]*[[:space:]]*:/ {
  sub(/[[:space:]]*:.*/, ""); if ($0 != "Axioms") print
}' Assumptions.log | LC_ALL=C sort -u > assumptions.txt
diff -u /fixture/wasmcert/allowed-axioms.txt assumptions.txt
start=$(date +%s%N)
opam exec -- coqchk -R "$upstream" Wasm -Q . '' Assumptions > kernel-check.log 2>&1
end=$(date +%s%N)
awk -v delta="$((end-start))" \
  'BEGIN { printf "coqchk\t%.3f\n", delta/1000000000 }' >> timings.tsv
cat timings.tsv
cat Assumptions.log
tail -3 kernel-check.log
printf 'PASS\n' > status.txt
trap - EXIT
