#!/bin/sh
# Copy the reviewable part of out/ (generated, ignored) into evidence/ (kept).
# Binaries stay in out/; their SHA-256 values are in the copied JSON files.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$here/out"
ev="$here/evidence"
rm -rf "$ev/correct" "$ev/faulty" "$ev/sunbeam"
mkdir -p "$ev"
cp "$out/toolchain.txt" "$ev/toolchain.txt"
for variant in correct faulty; do
  mkdir -p "$ev/$variant"
  cp "$out/$variant/extract/extraction.json" \
     "$out/$variant/harness/sunbeam-harness.json" \
     "$out/$variant/concrete-run.json" \
     "$out/$variant/target-function.wat" \
     "$out/$variant/timings.txt" \
     "$ev/$variant/"
  # The appended rule functions, as printed by wasm-tools (the original
  # module's functions are omitted; they are identified by hash).
  first=$(sed -n 's/^ *"first_rule_function_index": \([0-9]*\),$/\1/p' "$out/$variant/harness/sunbeam-harness.json")
  awk -v f="  (func (;$first;)" 'index($0, f) == 1 {keep=1} keep && /^  \((table|memory|global|export|elem|data) / {exit} keep' \
    "$out/$variant/harness/sunbeam-harness.wat" > "$ev/$variant/sunbeam-rules.wat"
done
if [ -d "$out/sunbeam" ]; then
  mkdir -p "$ev/sunbeam"
  cp "$out/sunbeam/summary.tsv" "$out/sunbeam/tools.txt" "$ev/sunbeam/"
  # The Prover's model for every run that has one: a counterexample for a
  # failed assert rule, a witness for a satisfied satisfy rule.
  find "$out/sunbeam" -name 'ctpp_*.txt' | while read -r f; do
    rel=${f#"$out/sunbeam/"}
    run=$(echo "$rel" | cut -d/ -f1-3 | tr / _)
    cp "$f" "$ev/sunbeam/model_$run.txt"
  done
  cp "$out/sunbeam/correct/trapAsAssert-z3/rule_split_1_conservation/run.log" "$ev/sunbeam/log_correct_trapAsAssert-z3_conservation.txt"
  cp "$out/sunbeam/faulty/trapAsAssert-z3/rule_split_1_conservation/run.log" "$ev/sunbeam/log_faulty_trapAsAssert-z3_conservation.txt"
  cp "$out/sunbeam/correct/trapAsAssert-z3/probe_oob_result_slot_returns/run.log" "$ev/sunbeam/log_correct_trapAsAssert-z3_probe_oob.txt"
fi

# Text traces are whitespace-normalized for review. The original reports in
# out/ remain untouched; no model value, instruction or verdict is changed.
python3 - "$ev/sunbeam" <<'PY'
import sys
from pathlib import Path
for path in Path(sys.argv[1]).glob('model_*.txt'):
    text = '\n'.join(line.rstrip() for line in path.read_text().splitlines())
    path.write_text(text.rstrip() + '\n')
PY
echo "evidence collected under $ev"
