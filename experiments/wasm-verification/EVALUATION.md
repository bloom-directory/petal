# Milestone 1: bytecode-verification backend spike

Date: 19 November 2026 (Asia/Bangkok). Host: Apple M1 Max, macOS (Darwin
25.6.0, arm64). Production SDK/builder sources are from `aaf171f`, with the isolated fixture
supplied by this change on `experiment/wasm-verification-tooling`.

Scope: the Milestone 1 benchmark only, as defined in [SPEC.md](SPEC.md). No
route-level, host, fuel, evidence-registry or lifecycle work.

## Summary

| | Certora Sunbeam | KWasm |
| --- | --- | --- |
| Target analyzed | Shipped function body, byte-identical inside a harness module that adds 5 imports and 9 rule functions | Shipped function body, decoded by KWasm's own decoder, inside a hand-written invocation configuration |
| Whole shipped module accepted | Yes (Chicory decoder; no decode or translation error) | **No**: pykwasm's decoder stops at `i64.reinterpret_f64` (0xBD), which KWasm's semantics also leaves unimplemented |
| SPLIT-1/2/3 (sum, order, gap; all `u64`) | Verified, assuming return (z3 and cvc5) | **Conditional, not proof success**, with an unproved rewrite; **not established** without it |
| SPLIT-4 normal return under PRE | **Not established**: no memory-bounds traps; the probe rule shows a normal return where Wasm traps | Direct return-state check passes in the trusted model without the custom rewrite; termination also uses the finite-body argument below |
| SPLIT-5 frame | Verified, assuming return | Verified in the trusted model (two claims, no custom rewrite) |
| Regressions R1–R3 | Verified (rules) | Exact-memory result is conditional on the unproved rewrite; also tested on the shipped module |
| Faulty variant | SPLIT-1, R2, R3 fail with models (consistent with total 1 for SPLIT-1); others hold | props and exact claims fail with residual `TOTAL = TOTAL/2 + TOTAL/2`; frame holds |
| Evidence type | Trusted-verifier verdict | Trusted-verifier verdict (K Haskell backend + z3); no proof certificate exported |
| Time per check | 4–6 s each, 72 runs in 6 min | 22–30 s each (amd64 emulation), 12 claims; exact timings in the retained TSV and Codex pipeline JSON |

Neither result is a kernel-checked certificate. Both rest on a trusted
extractor (this directory's `petal-wasm-evidence`) for the claim that the
analyzed body is the one in the packaged component.

**Decision: no-go for the complete guarantee with either backend alone.**
No backend is selected as meeting the full benchmark without an outstanding
obligation. Sunbeam has useful universal functional verdicts but omits finite
memory traps. KWasm has direct normal-return and frame checks in its trusted
model, but its functional result needs an unproved simplification, and its
whole-module decoder rejects the shipped module.

The missing capabilities are finite-memory/trap semantics in Sunbeam, and a
discharged shift/arithmetic lemma plus whole-module instruction support in
KWasm. Merely upstreaming an unproved rewrite does not discharge it. KWasm is
the more promising next research candidate for memory/ABI reasoning; this is
not an accepted complete proof or a commitment to Milestone 2.
[IRIS_REFERENCE.md](IRIS_REFERENCE.md) gives the requested prediction for
Iris-Wasm. No third backend was installed or integrated.

## Artifact identity

Built by `scripts/build-and-extract.sh` with the ordinary CLI:
`petal package --root split-even-petal --out <abs>/split-even.petal.tar.gz`
(this checkout's `petal`, wasm-tools 1.249.0, rustc/cargo 1.97.0). Repeated
runs reproduced both archives byte for byte (correct: three runs, faulty: two).

| | correct | faulty |
| --- | --- | --- |
| archive sha256 | `91e75727baa3b235339cf84250d8d1cb905566796bf0b8347d8a38a65af7cc30` | `664885c7667fb645a52bf5729c315843851c0f844792447daa29dc1df85b9611` |
| component `petal/split-even/split/[total].json.wasm` sha256 | `bbbf082cecd58c37f15b3ad72465007e95e75c01ee699d3ba851c51574e6ce7a` | `4ebe5e06398638a5055713a7a221c0d0c43046e6b375d1d1771168847f73f336` |
| core module 0 of 3 (`$main`) sha256 | `03e2854f303c7f978be5c3b358c8ae9b3d14652eada3023c6766bc2bafd5c4fb` | `ae239dc83f0dc6e609eada8e417eac15124222d02d3dc8b8dd5d174087d39a25` |
| target | export `bloom_verify_split_even` → func 67, type `(param i32 i64)` | same index and type |
| body sha256 | `c9c2e6618c6f89996bf63ba97fbd617af3e66eb91c63f10401e42ca13d4beba5` (26 bytes) | `f74678ec82cd2a52dfa2cc6b9e1144e41308db6b230a18e92c9b956adb3ecc70` |
| route caller | func 66 calls 67 directly | same |
| Sunbeam harness sha256 | `07f6a58ac1e0581e8a3bc4aaebfa7a7cbe0891db6da04d97cbfff6b3be0f2c4f` | `5d286d5cd9a88e447d2ff9dba743add4ed92ca613cba5621f23c0999d75cead7` |

Decoded correct body (wasmparser 0.249.0; pykwasm decodes the same 11
instructions, see [evidence/kwasm/correct-decoded.json](evidence/kwasm/correct-decoded.json)):

```wat
(func (;67;) (type 16) (param i32 i64)
  (local i64)
  local.get 0  local.get 1  i64.const 1  i64.shr_u  local.tee 2
  i64.store                       ;; [p]   := total >> 1
  local.get 0  local.get 1  local.get 2  i64.sub
  i64.store offset=8)             ;; [p+8] := total - (total >> 1)
```

The faulty body stores `total >> 1` at both offsets (and LLVM reorders the
stores): [evidence/faulty/target-function.wat](evidence/faulty/target-function.wat).
Straight-line: no loop, branch, call, `unreachable`, division or memory
growth (`possibly_trapping_or_nonstraight_opcodes: []`); the only possible
traps are the two stores.

## Pins and commands

Tested native host: Darwin arm64. Rust/cargo 1.97.0 (LLVM 22.1.6) and the
`wasm32-unknown-unknown` target are required. `build-and-extract.sh` refuses a
different compiler or wasm-tools version. The fixture and evidence helper
have separate Cargo.lock files; production dependencies are unchanged.

| Tool | Pin |
| --- | --- |
| wasm-tools | 1.249.0 (workspace metadata) |
| Extractor / encoder | wasmparser / wasm-encoder 0.249.0 |
| Concrete interpreter | wasmi 2.0.1 |
| CertoraProver / Sunbeam | 4ef546d00cd67d333914845dead2d72081e1f014 (8.20.3); Gradle wrapper 7.2 |
| Java | Temurin 21.0.13.1+2 |
| Native SMT solvers | Z3 5.3.3; cvc5 1.4.2 |
| Demangler | rustfilt 0.2.1 |
| KWasm / pykwasm | v0.1.157, 212271bd434bd402e27c42f6416854342733386d |
| K / Python kframework | 7.1.329; Python dependencies in `kwasm/requirements.lock` |
| Container Z3 | 4.12.1 (the older KWasm README recommends 4.8.15) |

K image manifest:
`runtimeverificationinc/kframework-k:ubuntu-jammy-7.1.329@sha256:4864e4dcef177614d9b4fc3c787932ba1d5ca579366af609f0707e6e5d547b4b`.
The local amd64 image ID and actual solver versions are recorded in
`evidence/kwasm/tools.txt`. The native release assets have full SHA-256 values
in `scripts/tool-assets.sha256`; the actual Sunbeam jar hash is recorded in
`evidence/sunbeam/tools.txt`. No reproducible-jar claim is made.

From this directory, with git, GitHub CLI, curl, unzip, Python 3, Rust 1.97.0
and Docker available:

```sh
scripts/setup-sunbeam.sh
. /tmp/petal-m1/env.sh
scripts/build-and-extract.sh
scripts/run-sunbeam.sh

# Fresh setup; omit clone/checkout if the exact pinned checkout already exists.
git clone --no-checkout https://github.com/runtimeverification/wasm-semantics.git /tmp/petal-m1/src/kwasm
git -C /tmp/petal-m1/src/kwasm checkout --detach 212271bd434bd402e27c42f6416854342733386d
KWASM_SRC=/tmp/petal-m1/src/kwasm scripts/run-kwasm.sh
scripts/collect-evidence.sh
python3 -m unittest discover -s scripts -p 'test_verdicts.py'
python3 scripts/check-evidence.py
```

The bootstrap supports the tested macOS/arm64 host and uses isolated roots
under `/tmp/petal-m1`. It was smoke-tested using the provisioned assets, not
rerun on a fresh machine. KWasm uses linux/amd64 emulation on this host. The
runner checks the image, source revision, output mount and Python dependency
lock before reusing a container. Remove it after use with
`docker rm -f petal-m1-kwasm`.

Java uses `-Djava.awt.headless=true -Dapple.awt.UIElement=true`, including in
the isolated environment for child processes, to avoid stealing keyboard focus.
A direct Sunbeam invocation is
`java -Djava.awt.headless=true -Dapple.awt.UIElement=true -jar "$CERTORA/emv.jar" sunbeam-harness.wasm -rule <rule> -wasmHost NONE -solver '[z3:def]' -trapAsAssert true`.
No production runtime, sandbox settings, dependencies or release workflows
were changed.

## Certora Sunbeam

### What was run

The Sunbeam workflow expects property rules compiled into the analyzed
binary (CVLR `cvt` feature builds). That would analyze a different build, so
it was not used. Instead `petal-wasm-evidence harness` re-encodes the
extracted module (wasm-encoder 0.249.0) with five added `env.CVT_*` imports
and nine exported rule functions
([evidence/correct/sunbeam-rules.wat](evidence/correct/sunbeam-rules.wat)).
Imports shift every defined function index by five. Bodies without function
references (28 of 156, including the target) are copied byte for byte; the
other 128 are re-encoded with shifted `call` indices. The tool re-decodes the
harness and refuses to write it unless the target body is byte-identical, its
type is unchanged and the original export points at it.

Rules call the target with `p` nondeterministic under `p ≤ 2^32 − 16` and
`total` nondeterministic (or fixed for regressions), then assert the
bit-vector encodings in SPEC.md. Matrix: 2 variants × 9 rules × 2 trap modes
× 2 solvers = 72 runs; [evidence/sunbeam/summary.tsv](evidence/sunbeam/summary.tsv).

### Results

Every one of the 72 runs gave a definite verdict; z3 and cvc5 agree on all
of them, and `-trapAsAssert true` changes nothing (the body has no
`unreachable`).

| Rule | correct | faulty |
| --- | --- | --- |
| `rule_split_1_conservation` | SUCCESS | **FAIL** (model consistent with total 1, p 0) |
| `rule_split_2_ordered` | SUCCESS | SUCCESS |
| `rule_split_3_gap` | SUCCESS | SUCCESS |
| `rule_split_4_reaches_return` (satisfy) | SUCCESS (witness) | SUCCESS |
| `rule_split_5_frame` | SUCCESS | SUCCESS |
| `rule_split_r1_zero` | SUCCESS | SUCCESS (0 → (0, 0) is right for both) |
| `rule_split_r2_odd` (7) | SUCCESS | **FAIL** |
| `rule_split_r3_max` | SUCCESS | **FAIL** |
| `probe_oob_result_slot_returns` (satisfy, p = 0xFFFFFFF0) | **SUCCESS** | SUCCESS |

The call trace shows Sunbeam inlining `$bloom_verify_split_even`, and the
optimized TAC has the store/shift/subtract structure between
`wasm.inline.start` and `wasm.inline.end`. The faulty variant fails exactly
the properties its fault breaks.

The probe is the key negative finding. With the result slot at 0xFFFFFFF0,
the first `i64.store` is outside any memory smaller than 4 GiB; wasmi traps
(`out of bounds memory access`, in
[evidence/correct/concrete-run.json](evidence/correct/concrete-run.json)).
Sunbeam finds a normal-return witness, in both trap modes.

### Coverage and semantics, from source (`src/main/kotlin/wasm`)

- Decoder: Chicory. The whole shipped module was accepted by the decoder,
  including instructions outside the target. Only reachable rule/target
  instructions are checked semantically; this does not verify every other
  function in the module. No unsupported-instruction error occurred in this run.
- Loads and stores (`WasmImpInstr.kt`, `Load`/`Store`) become TAC
  `ByteLoad`/`ByteStore` on an unbounded byte map. There is no bounds check
  and no memory size.
- `i32/i64.div_u`/`rem_u` become TAC `Div`/`Mod` (`NumericExpr.kt`) with no
  zero-divisor trap. The signed forms likewise.
- `unreachable` is the only core-Wasm trap (`Control.Unreach` → `Trap.trap`).
  Traps become `assume false` (revert) by default and `assert false` with
  `-trapAsAssert`.
- Compiler builtins (`memcpy`, `__rust_alloc`, `__multi3`, …) are replaced
  by summaries when a local function has that name. The shipped module is
  symbol-stripped, so none matched here. Elsewhere this would silently
  replace code with a model.
- Loops are unrolled to a bound; this body has none. The `-ruleSanityChecks
  basic` vacuity checks produced no output for the Wasm flow, so the satisfy
  rule and the faulty variant are the only non-vacuity evidence.

### Trusted computing base

This directory's extractor and harness encoder (wasmparser/wasm-encoder
0.249.0); the nine hand-written rule functions (their correspondence to
SPEC.md is reviewed, not checked); Chicory; Sunbeam's Wasm-to-TAC
translation and its memory and arithmetic model; the TAC optimizer
(including `tac_optimizer`, built from `fried-egg`); Certora's VC
generation; z3 5.3.3 or cvc5 1.4.2. The verdict is a trusted-verifier
verdict; no certificate is produced. The Prover reports its version as
`4ef546d00c.dirty` because of untracked build outputs only (`git status`
lists build directories, no tracked changes).

### Effort and time

About 13 minutes from cloning to the first verdict, 7 min 31 s of it the
Gradle build. Each run: 4–6 s, mostly JVM start-up and translation of the
whole module. Extractor and harness tool: about 950 lines of Rust, shared
with extraction and the concrete runs.

## KWasm

### Why it was evaluated

Sunbeam cannot express SPLIT-4, so its assurance model is unsuitable for the
required property. That is the condition for trying the second backend.

### What was run

1. `wasm2kast` (pykwasm's binary decoder) on each whole extracted module:
   **fails**, `WasmParseError: Unsupported opcode: 0xbd` (`i64.reinterpret_f64`,
   used twice by other functions of the module, not by the target). KWasm's
   semantics also lists `inn.reinterpret_fnn` and `fnn.reinterpret_inn` as
   "TODO: Unimplemented" (`numeric.md`). So the shipped module cannot be
   loaded, instantiated or executed in KWasm. Ordinary Rust Petals that
   link float formatting are likely to hit the same gap.
2. Function-level instead: [kwasm/gen_claims.py](kwasm/gen_claims.py)
   checks the module hash and the recorded body bytes at their offset, then
   decodes those bytes with pykwasm's own locals and `expr` decoders. It
   writes K all-path reachability claims whose `<fCode>` is that decoding
   (`#instrWithPos` wrappers and offsets kept). The initial state is a store
   with memory 0 of `SIZE` pages, arbitrary contents `BM`, arbitrary
   `FADDR`/`CUR`/`MEMADDR`/value stack/locals, and `P`, `TOTAL` on the stack.
   The precondition is PRE (`P + 16 ≤ SIZE · 65536`) plus the operand
   ranges. The target is `.K` with the arguments popped, which is a normal
   return.
3. `kwasm prove` per claim, for both variants.

### Results ([evidence/kwasm/summary.tsv](evidence/kwasm/summary.tsv))

| Claim | Property | correct | faulty |
| --- | --- | --- | --- |
| `returns` | Normal-return target state, no custom lemma | VERIFIED_IN_TRUSTED_MODEL | VERIFIED_IN_TRUSTED_MODEL |
| `props` | P-SUM + P-ORD + P-GAP with a return-state target, KWasm lemmas only | NOT_ESTABLISHED | NOT_ESTABLISHED |
| `props-lemma` | same, plus an unproved rewrite | **CONDITIONAL_UNPROVED_LEMMA** | NOT_ESTABLISHED: residual `TOTAL = TOTAL/2 + TOTAL/2` |
| `frame-below` | P-RET + P-FRAME, `Q + 1 ≤ P` | VERIFIED_IN_TRUSTED_MODEL | VERIFIED_IN_TRUSTED_MODEL (the fault does not touch the frame) |
| `frame-above` | P-RET + P-FRAME, `P + 16 ≤ Q < SIZE · 65536` | VERIFIED_IN_TRUSTED_MODEL | VERIFIED_IN_TRUSTED_MODEL |
| `exact-lemma` | P-RET + final memory `= setRange(setRange(BM, P, TOTAL/2, 8), P+8, TOTAL − TOTAL/2, 8)` | **CONDITIONAL_UNPROVED_LEMMA** | NOT_ESTABLISHED |

The `props` check without the custom rewrite leaves residual obligations
about shift ranges, memory readback and arithmetic. K exposes `>>Int` to Z3
as an uninterpreted function. The custom rule
`X >>Int 1 => X /Int 2 requires 0 <=Int X [simplification]`
is **not proved by K**. A `#Top` result using it is conditional and does not
count as a discharged functional proof. Failed reachability checks are
residual obligations, not automatically counterexamples. The confirmed
negative control is Sunbeam's models plus concrete execution.

The direct `returns` checks reach a normal-return target state without the
custom rewrite or an arithmetic postcondition. K reachability is not a
general termination checker. For this body Codex separately reviewed the
finite decoded sequence: no loops, branches, calls, memory growth or trapping
arithmetic; both stores fit under PRE; every other operation is total. Thus
normal return follows for this sequence with enough resources for the call.
A body change requires fresh review. This does not establish a Bloom fuel or
availability bound. `check-evidence.py` guards the restricted instruction set
used by that argument; it is not an additional verification backend.

### Trusted computing base

This directory's extractor (wasmparser) for the module, body offset and
type; pykwasm 0.1.157's instruction and locals decoder (independent of
wasmparser; both decoders agree on the 11 instructions); the hand-written
part of the claim (`<fType>` from the extracted type, the invocation
configuration, the property encodings); the KWasm semantics
(`wasm.md`, `numeric.md`, `wasm-data.md`) and `kwasm-lemmas.md` (all
`[simplification]` rules are trusted); the custom rewrite as an additional **open** obligation for conditional results; the K frontend
and Haskell backend (kore-exec/booster 7.1.329); z3 4.12.1. No proof object
is exported or checked independently. KWasm's semantics is not mechanically
related to the Wasm specification; its fidelity comes from testing against
the spec test suite.

Not modelled by these claims: module instantiation, the other 155 functions,
globals and tables (unconstrained but unused), and the host. The claims range over stores with that memory shape, broader than the real
instance. The bytes-to-invocation-state correspondence is trusted, not proved.

### Effort and time

About 15 minutes from cloning to the first successful trusted-model check. Image pull: 105 s,
3.5 GB. `kdist build wasm-semantics.kwasm-lemmas`: 40 s of
kompile. Each claim: 22–30 s, under amd64 emulation on arm64. Claim
generator: about 200 lines of Python. The obstacles were the claim syntax
(pyk prints `#instrWithPos` in a form the parser rejects), the
module-name/file-name rule, the uninterpreted shift and the disjunctive frame
precondition.

## Evidence types kept distinct

| Evidence | Where | What it covers |
| --- | --- | --- |
| Source tests | `split-even-petal/route/src/tests.rs`, `cargo test` | Rust semantics of `split_even` on 10 inputs; not the shipped Wasm |
| Shipped-module tests | `evidence/*/concrete-run.json`, wasmi 2.0.1 | 6 totals × 3 result addresses on the extracted module; full-memory frame check; out-of-bounds trap |
| Trusted-verifier verdict | Sunbeam (`evidence/sunbeam/`), KWasm (`evidence/kwasm/`) | As tabulated above, under each TCB |
| Kernel certificate | none | Iris-Wasm/WasmCert remain the reference for this; not attempted |
| Source proof | none | — |

## Independent review and remaining limitations

[CODEX_REVIEW.md](CODEX_REVIEW.md) records Codex's independent code/specification
review, corrections and checks. Codex rebuilt both final archives identically,
cross-checked extraction with pinned `wasm-tools component unbundle` (which
shares parser code), reran 12 Sunbeam functional checks with both solvers, and
ran direct KWasm return-state checks without the custom rewrite. The retained
review JSON files include timings and identities.

Remaining: no complete single-backend guarantee; no verified extraction,
state-construction or component-wiring theorem; no kernel certificate; no
negative control for the frame property; no host, route, fuel or lifecycle
verification. Source proofs are not claimed. The first Claude Opus 5.5 CLI
pass took 60 min 39 s, including research/setup/proof work and reruns. Codex
review overlapped that work; measured additional runs are recorded separately
in `evidence/codex-*.json`, not represented as human proof-development hours.
