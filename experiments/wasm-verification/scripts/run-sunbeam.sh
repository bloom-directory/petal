#!/bin/sh
# Milestone 1: run Certora Sunbeam (local, open-source Certora Prover) on the
# harness built from each packaged variant by scripts/build-and-extract.sh.
#
# Usage: CERTORA=<dir with emv.jar> scripts/run-sunbeam.sh
# Requires java (JDK 19+), z3, cvc5 and rustfilt on PATH. See EVALUATION.md.
#
# Each run is one rule, one trap mode and one solver. The verdict is read from
# the Prover's Reports/output.json; anything other than SUCCESS or FAIL, or
# a missing file, is recorded as INCONCLUSIVE.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$here/out"
: "${CERTORA:?set CERTORA to the directory holding emv.jar}"
jar="$CERTORA/emv.jar"
test -f "$jar"

rules="rule_split_1_conservation rule_split_2_ordered rule_split_3_gap
rule_split_4_reaches_return rule_split_5_frame rule_split_r1_zero
rule_split_r2_odd rule_split_r3_max probe_oob_result_slot_returns"

summary="$out/sunbeam/summary.tsv"
mkdir -p "$out/sunbeam"
{
  echo "emv_jar_sha256: $(shasum -a 256 "$jar" | cut -d' ' -f1)"
  java -Djava.awt.headless=true -Dapple.awt.UIElement=true -version 2>&1
  z3 --version
  cvc5 --version | head -1
  rustfilt --version 2>/dev/null || echo "rustfilt present"
} > "$out/sunbeam/tools.txt"
printf 'variant\tharness_sha256\trule\ttrap_mode\tsolver\tverdict\texit\tseconds\n' > "$summary"

for variant in correct faulty; do
  harness="$out/$variant/harness/sunbeam-harness.wasm"
  sha=$(shasum -a 256 "$harness" | cut -d' ' -f1)
  for mode in default trapAsAssert; do
    case $mode in
      default) trap_args="" ;;
      trapAsAssert) trap_args="-trapAsAssert true" ;;
    esac
    for solver in z3 cvc5; do
      for rule in $rules; do
        dir="$out/sunbeam/$variant/$mode-$solver/$rule"
        rm -rf "$dir"
        mkdir -p "$dir"
        cp "$harness" "$dir/sunbeam-harness.wasm"
        start=$(date +%s)
        set +e
        # shellcheck disable=SC2086
        (cd "$dir" && java -Djava.awt.headless=true -Dapple.awt.UIElement=true -jar "$jar" sunbeam-harness.wasm -rule "$rule" -wasmHost NONE \
          -solver "[$solver:def]" $trap_args > run.log 2>&1)
        code=$?
        set -e
        seconds=$(( $(date +%s) - start ))
        report=$(ls -d "$dir"/emv-*/Reports 2>/dev/null | head -1)
        verdict=$(python3 "$here/scripts/verdicts.py" sunbeam "${report:-$dir/missing}/output.json" "$rule" "$code")
        rm "$dir/sunbeam-harness.wasm"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$variant" "$sha" "$rule" "$mode" "$solver" \
          "$verdict" "$code" "$seconds" >> "$summary"
        echo "$variant $mode $solver $rule: $verdict ($code, ${seconds}s)"
      done
    done
  done
done
