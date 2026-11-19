# WasmCert kernel-checked proof of the packaged split-even function

This extends the existing experiment with direct Rocq proofs using WasmCert.
It introduces no Iris integration and no production SDK dependencies. The
existing Sunbeam evidence remains a separate result.

**Outcome:** the good-function theorem and faulty-artifact counterexample both
compile and pass an independent `coqchk` recheck in a fresh image built by the
documented Dockerfile. [Retained results](evidence/result.json) record the exact
artifact identities, proof inputs, toolchain, assumptions, and checks.

## What is proved

`Execution.packaged_correct_function` combines decoding of the actual selected
function bytes with `function_guarantee`. For every i64 bit pattern, arbitrary
initial memory contents, and a valid invocation satisfying `p + 16 <= L`, the
proof constructs a finite WasmCert execution of `call 67` to a normal return:

- The two output cells contain the little-endian encodings of
  `total >> 1` and `total - (total >> 1)`.
- Their unsigned values conserve the total, are ordered, and differ by at
  most one. These properties use mathematical integers, with representability
  proved; wrapping arithmetic cannot satisfy an incorrect conservation claim.
- Memory length and bytes outside the 16-byte output slot are preserved.
- The caller frame and host state are preserved. No host call is used.

The invocation theorem includes the call, native-function frame construction,
body steps, label removal, and normal return. Store success is derived from
the memory bound, not assumed. No alignment precondition is required.
This is a constructive normal execution for each valid invocation in this
call-free deterministic fragment, not an existential witness for one input
or a postcondition conditional on returning. Runtime fuel/interruptions and
native execution resources remain outside the semantic termination claim.

`Arithmetic.v` includes exact zero, 1, 7, and `u64::MAX` regressions. Those
computations alone are arithmetic checks; the universal execution theorem is
what connects the arithmetic to the emitted function.

`Negative.faulty_violates_SPLIT_1` decodes the separately packaged faulty body
and constructs its execution at total 1: it writes `(0, 0)`, violating
conservation. The negative test uses the faulty artifact's own identity and
instructions, rather than rejecting an old hash. It covers the body execution;
the extra call/return wrapper is proved for the good variant.

## Files and proof chain

| File | Responsibility |
| --- | --- |
| `generate_artifact.py` | Check artifact identities and packaged component bytes; embed the unchanged core modules and selected body offsets |
| `Artifact.v` | Slice those embedded module bytes and prove WasmCert decodes the selected locals and instructions to the code used in the execution proofs |
| `Arithmetic.v` | Prove the unsigned-shift/division identity, nonwrapping subtraction, conservation, order, gap, and regressions |
| `MemoryFacts.v` | Derive successful bounded stores, output-byte contents, and preservation from WasmCert's memory-interface laws |
| `Execution.v` | Construct the finite body and native call/return execution and combine the complete guarantee with decoding |
| `Negative.v` | Construct the faulty function's execution and conservation counterexample |
| `Assumptions.v` | Print transitive assumptions after section parameters become universally quantified |
| `allowed-axioms.txt` | Fail-closed audit of the observed upstream assumptions; not a replacement for kernel checking |
| `Dockerfile`, `toolchain.opam.lock` | Pin the proof environment separately from production dependencies |
| `check.sh` | Compile every proof, audit assumptions, and independently recheck the proof objects with `coqchk` |

The checked object remains core export `bloom_verify_split_even`, function 67
(defined function 66), type `(param i32 i64)` with no direct results. Both
variants have their own component/module/body identities. The complete hashes
are retained in `evidence/identities.json`; they match the existing experiment.
The normal SDK and `petal package` workflow produced these artifacts.

### Precisely which decoding is checked

Rocq computes `firstn body_length (skipn body_offset module_bytes)` and runs
WasmCert's `parse_code_func` on that exact slice, including local declarations
and the function's final `end`. The parser's runner requires complete input
consumption. The resulting code is proved equal to the code used by the
execution theorem. There is no re-encoded harness or separately compiled test
implementation in this chain.

The existing extractor remains trusted to select the core module, export,
function offset, function type, and memory/ABI context. Component extraction
and module-wide structural validation are not proved inside Rocq. A full
`run_parse_module` computation was attempted but stopped while unfinished
because it was expensive on the generated module. It is **not** counted as a
successful whole-module decoding proof. The narrower body decoding proof
retains this selection boundary explicitly.

## Reproduction

Prerequisites: Docker with linux/amd64 support, GitHub network access for initial
setup, Python 3, and the existing experiment's Rust/SDK build prerequisites.
On arm64 the proof image runs under amd64 emulation; no sandbox or memory/fuel
limits are changed.

From `experiments/wasm-verification`:

```sh
# Produce the same artifacts through the existing ordinary package workflow.
# Requires Rust 1.97.0 and wasm-tools 1.249.0 as documented in ../EVALUATION.md.
sh scripts/build-and-extract.sh

# Build the isolated proof toolchain once, then check the proofs.
sh wasmcert/setup.sh
sh wasmcert/run.sh

# Integration checks for stale or mismatched input evidence, even with -O.
python3 -O -m unittest discover -s wasmcert -p test_artifact.py -v
```

The proof baseline is WasmCert commit
`58a3d3f8b7af4dc8aa9351f254a9e672643b21f8` (2.2.1), Rocq 9.0.1,
OCaml 4.14.2, Dune 3.21.1, CompCert 3.17, Flocq 4.2.2,
MathComp ssreflect 2.4.0, and rocq-parseque 0.3.1. The base-image digest and
all resolved package versions/source metadata are pinned in the Dockerfile
and frozen opam switch export.

Output is under `out/wasmcert/`: proof objects, complete assumption reports,
kernel-check log, identity/source hashes, tool versions, timings, and status.
`PASS` is written only after compilation, the axiom audit, and `coqchk` all
succeed. A failure leaves `FAILED`; unsupported decoding, failed proofs,
interruption, and incomplete checks never become proof success. Retained
outcomes are in `evidence/` alongside this document.

## Assumptions and coverage

The claim is **kernel-checked compiled-Wasm verification under explicit
assumptions**, not an axiom-free theorem or an end-to-end proof of Bloom.

The observed upstream logical assumptions are:

- `Classical_Prop.classic`;
- `FunctionalExtensionality.functional_extensionality_dep`;
- `ClassicalDedekindReals.sig_not_dec`;
- `ClassicalDedekindReals.sig_forall_dec`.

The operational semantics also mentions five abstract SIMD functions under
`simd_execute.SIMD_ops`: `app_vunop_str`, `app_vbinop_str`,
`app_vternop_str`, `app_vtestop_str`, and `app_vshiftop_str`. They appear in
the transitive assumption report because the full semantics includes SIMD.
This function contains and executes no SIMD instructions; no claim about those
operations is proved. The arithmetic theorem itself is closed under the global
context. No target-specific axiom, admission, solver oracle, or custom execution
semantics is introduced.

Memory and host interfaces are universally quantified parameters. Memory
operations must satisfy WasmCert's `BlockUpdateMemory` laws. Initial function
and memory lookups, the ABI arguments, and an in-bounds output slot are explicit
preconditions. Module instantiation and the route's provision of that slot are
not established here.

Outside the checked theorem we trust artifact extraction/embedding and selection,
the experimental WasmCert parser's correspondence to the official binary
format, the applicability of WasmCert's semantics, Rocq's kernel/checking
environment, and the runtime's correspondence to those semantics. Kernel
checking the decoder's result does not prove parser correctness against the
standard. Concrete decoding and regression lemmas use `vm_compute`; Rocq's
conversion/reduction machinery is part of the trusted checking environment.
The extracted OCaml Wasm interpreter is not used as proof evidence. Rust
compiler correctness is not assumed: the actual emitted body is
the proof target.

The public route, JSON encoding, canonical/component ABI adapters, host behavior,
deployment identity enforcement, fuel/cost bounds, settlement, and external
effects are not covered. Iris, ZK wrapping, arbitration, signing, and registries
are not implemented.

## Assessment

Direct WasmCert proofs are sufficient for this small straight-line function;
Iris is not required. The cost is manual proof construction and substantial
one-time proof-library setup. Generalization to larger Rust-generated routes,
loops, host calls, or automatic proof generation remains untested. The body
selection boundary and runtime/invocation assumptions remain material.

Proof development and provisioning were interleaved; separate human authoring
time was not measured. Per-stage verification timings are recorded by the
runner. The unfinished full-module parser attempt is a tooling limitation,
not evidence of correctness or a semantic counterexample.

The successful fresh-image run took 18.115 seconds to compile the proof inputs
and print/audit assumptions, followed by 51.412 seconds for `coqchk`: 69.527
seconds total, excluding image setup and artifact generation. Per-stage values
are in [evidence/timings.tsv](evidence/timings.tsv). The proof development is
approximately 630 lines of Rocq, including reusable memory/context lemmas,
artifact binding, assumption reporting, and the negative control.

Review checked that the final theorem includes decoding and all five properties,
that successful stores follow from the bound, and that the call proof includes
administrative return steps. The decoder equality caught an initial reversal
of the `memarg` constructor's offset/alignment fields; the corrected proof uses
the exact decoded arguments. The axiom guard checks the final theorems after
section closure, so memory/host parameters are quantified rather than mistaken
for discharged obligations. Three artifact-binding integration tests pass even
with Python optimization enabled; the existing verdict tests and Sunbeam
evidence checker also pass.
