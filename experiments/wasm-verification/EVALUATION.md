# Milestone 1: Sunbeam for packaged Rust-generated Wasm

19 November 2026. Tested on Apple M1 Max, Darwin 25.6.0, arm64.
Production SDK, builder and CLI sources are unchanged from
`aaf171f6ed4542ee7f5ba3f4665c9e6f4abf4288`.

## Outcome and guarantee

**Milestone 1 is achieved:** the experiment demonstrates universal functional
verification of an actual Rust-generated function embedded in a normally
packaged Petal and selects Sunbeam as the provisional backend. The complete
benchmark claim combines trusted-verifier functional results with a reviewed
bytecode argument for normal return, the memory frame and model correspondence.
This is a function-level tooling decision under explicit trust assumptions.

For every u64 total and valid invocation with an in-bounds 16-byte result
slot, the function returns normally, preserves memory outside that slot,
and produces ordered shares whose mathematical sum is the total and whose
difference is at most one. [SPEC.md](SPEC.md) states the exact requirements,
precondition and resource exclusions. [ARGUMENT.md](ARGUMENT.md) explains why
the pieces establish this guarantee without assuming that the function returns.

The arithmetic checks are automated. The normal-return and model-correspondence
argument is manually reviewed. No independently kernel-checked certificate,
Rust-source proof or verified extraction/compiler theorem is claimed.

## Artifact identity

Both variants use the existing SDK and ordinary `petal package`, which builds
and checks the route components. Extraction reads
`petal/split-even/split/[total].json.wasm` from the resulting archive.

| SHA-256 | Correct | Faulty |
| --- | --- | --- |
| Archive | `91e75727baa3b235339cf84250d8d1cb905566796bf0b8347d8a38a65af7cc30` | `664885c7667fb645a52bf5729c315843851c0f844792447daa29dc1df85b9611` |
| Component | `bbbf082cecd58c37f15b3ad72465007e95e75c01ee699d3ba851c51574e6ce7a` | `4ebe5e06398638a5055713a7a221c0d0c43046e6b375d1d1771168847f73f336` |
| Core module 0 of 3 | `03e2854f303c7f978be5c3b358c8ae9b3d14652eada3023c6766bc2bafd5c4fb` | `ae239dc83f0dc6e609eada8e417eac15124222d02d3dc8b8dd5d174087d39a25` |
| Function body | `c9c2e6618c6f89996bf63ba97fbd617af3e66eb91c63f10401e42ca13d4beba5` | `f74678ec82cd2a52dfa2cc6b9e1144e41308db6b230a18e92c9b956adb3ecc70` |
| Sunbeam harness | `07f6a58ac1e0581e8a3bc4aaebfa7a7cbe0891db6da04d97cbfff6b3be0f2c4f` | `5d286d5cd9a88e447d2ff9dba743add4ed92ca613cba5621f23c0999d75cead7` |

The checked object is core export `bloom_verify_split_even`, function 67,
type `(param i32 i64)` with no direct results. The C ABI writes the two u64
shares at `p` and `p+8`. Function 66 calls this same function. The decoded
correct body is:

```wat
(func (param i32 i64) (local i64)
  local.get 0  local.get 1  i64.const 1  i64.shr_u  local.tee 2
  i64.store
  local.get 0  local.get 1  local.get 2  i64.sub
  i64.store offset=8)
```

The faulty body stores `total >> 1` at both offsets, in reverse store order.
It is separately packaged, extracted and analyzed under its own identity.
The negative test is an actual failed property check and concrete wrong
result, not rejection of an old hash.

Both final archives were independently rebuilt identically. Extraction was
cross-checked against pinned `wasm-tools component unbundle`; it shares parser
code with the helper, so this is a cross-check, not a verified byte-binding
theorem. Full identity records and body bytes are in `evidence/*/extraction.json`.

## Verification results

The Sunbeam harness adds five verification imports and nine rules to the
extracted module. Defined function indices shift by five. The target contains
no function-index references, so its body is copied verbatim. The helper
re-decodes the result and checks body equality, type and export binding.
Other functions are re-encoded as necessary for reference shifts. No separately
compiled implementation substitutes for the packaged function.

Rules call the target for arbitrary u64 inputs. The conservation encoding
includes `first <= total` before interpreting `second == total - first`;
the ordered-gap encoding includes `first <= second` before subtraction.
These statements establish the mathematical-integer properties without
modular-overflow loopholes. The exact equivalences are documented in SPEC.

The retained matrix contains 72 runs: 2 variants x 9 rules x 2 trap modes x
Z3/cvc5. Every row has a consistent report and process exit status. Missing,
unsupported, unknown, timed-out or incomplete results are never counted as
proof success. The reporting tests cover inconsistent exits and missing or
unrecognized results.

| Rule | Correct | Faulty |
| --- | --- | --- |
| Conservation, all u64 | SUCCESS | FAIL |
| Ordering, all u64 | SUCCESS | SUCCESS |
| Gap, all u64 | SUCCESS | SUCCESS |
| Frame in Sunbeam's model | SUCCESS | SUCCESS |
| Exact zero regression | SUCCESS | SUCCESS |
| Exact 7 and u64::MAX regressions | SUCCESS | FAIL |
| Return-reachability witness | SUCCESS | SUCCESS |
| Out-of-bounds return witness | SUCCESS (model limitation) | SUCCESS (model limitation) |

Both solvers and both trap modes agree. The faulty conservation model is
consistent with total 1. Existential witnesses do not prove universal normal
return. The out-of-bounds witness deliberately demonstrates the model gap.
Independent reruns of the three functional properties on both variants with
both solvers produced 12 matching verdicts.

Concrete execution uses the original extracted modules in wasmi, with every
host import trapping if reached: 6 totals x 3 result addresses, including an
unaligned slot and the last valid slot. Correct passes 18/18. Faulty has 9
expected odd-total conservation/exact-result failures, including `7 -> (3,3)`.
Both trap on the out-of-bounds regression. Host Rust tests also cover zero,
odd/even totals and u64::MAX. Tests are distinct from universal verification.

## Trust assumptions and boundaries

The guarantee trusts archive extraction and Wasm decoding/validation; the
body-preserving harness transformation and property encodings; Sunbeam's
relevant instruction semantics, Wasm-to-TAC translation, optimizations and
VC generation; and Z3 or cvc5. It also relies on the reviewed argument that
valid concrete executions have the same observable results in the model.
No proof certificate checks those components independently.

Sunbeam is run with `-wasmHost NONE`. The selected function has no calls,
imports, globals/table accesses or reads of initial memory. No Soroban host
model or local-function summary is needed for it. Decoding the whole module
does not semantically verify all of its other functions.

The invocation assumes an already instantiated module, the ABI operands and
in-bounds result slot specified by PRE, and resources for the finite call
without interruption. Normal return itself is derived from the decoded
instructions. No Bloom fuel, availability, public-route parsing, JSON,
component/canonical-ABI wiring, initialization or host-effect guarantee follows.

## Sunbeam gaps and potential Bloom alignment

Sunbeam's load/store model omits finite-memory bounds traps. At
`p=0xfffffff0`, it finds a returning witness even though the module's 17-page
concrete memory would trap. `trapAsAssert` changes handling of modelled traps;
it does not introduce the missing bounds checks. In the tested source,
`memory.size` and `memory.grow` also lack a consistent current-length model.
For this finite, call-free function, PRE and instruction review close the
normal-return obligation as explained in ARGUMENT. This does not establish
that the same manual approach scales to larger routes.

Potential later work, none implemented here:

- Model memory length, non-wrapping access bounds and relevant trap behavior,
  with fail-closed handling of unsupported memory operations.
- Specify Bloom host contracts for the calls a route actually reaches,
  including failures and effects, preserving Broker-owned authorization.
- Connect those contracts to the WIT/SDK ABI. Sunbeam has an internal
  `WasmHost` interface, but host selection is compiled in; an upstream
  extension point could help keep Bloom models separate.
- Audit relevant summaries/optimizations and preserve exact packaged-artifact
  binding as coverage grows.

A development fork and the next route-level commitment gate are deferred.
No Bloom runtime, sandbox, signing, evidence-registry or lifecycle changes
are part of Milestone 1. Iris-Wasm remains a reference option for stronger
certificates; see [IRIS_REFERENCE.md](IRIS_REFERENCE.md).

## Pins and reproduction

| Tool | Pin |
| --- | --- |
| rustc / cargo | 1.97.0; LLVM 22.1.6; full metadata in `evidence/toolchain.txt` |
| wasm-tools | 1.249.0, matching workspace metadata |
| Extractor / encoder | wasmparser / wasm-encoder 0.249.0; separate Cargo.lock |
| Concrete interpreter | wasmi 2.0.1; separate Cargo.lock |
| Sunbeam / CertoraProver | 4ef546d00cd67d333914845dead2d72081e1f014 (8.20.3); Gradle wrapper 7.2 |
| Java | Temurin 21.0.13.1+2 |
| SMT solvers | Z3 5.3.3; cvc5 1.4.2 |
| Demangler | rustfilt 0.2.1 |

From this directory, with Rust 1.97.0 and its `wasm32-unknown-unknown` target,
git, GitHub CLI, curl, unzip and Python 3 available:

```sh
scripts/setup-sunbeam.sh
. /tmp/petal-m1/env.sh
scripts/build-and-extract.sh
scripts/run-sunbeam.sh
scripts/collect-evidence.sh
python3 -m unittest discover -s scripts -p 'test_verdicts.py'
python3 scripts/check-evidence.py
```

The bootstrap supports the tested macOS/arm64 host, checks the full native
asset hashes in `scripts/tool-assets.sha256`, and checks out the exact
prover revision. Tools and Gradle live under `/tmp/petal-m1`; production SDK
dependencies are unchanged. The bootstrap was smoke-tested with provisioned
assets, not installed again on a clean machine. The actual Sunbeam jar hash
is retained in `evidence/sunbeam/tools.txt`; no reproducible-jar claim is made.
Java and child processes run headlessly to avoid stealing keyboard focus.

## Effort and timings

Initial Sunbeam setup to the first verdict took about 13 minutes, including
7 min 31 s for the prover build. Extraction/harnessing/execution uses roughly
950 Rust lines plus shell/Python reporting scripts. The reviewed bytecode
argument is a separate short document. Proof authoring was iterative; an
isolated authoring-time measurement was not recorded.

Sunbeam runs typically take 4–6 seconds each, mostly startup/translation.
Per-run timings are in `evidence/sunbeam/summary.tsv`. Package timings are in
`evidence/*/timings.txt`; an independent rebuild took about 20 seconds in the
recorded environment. The Sunbeam-only reproduction rebuilt/package-tested in 23.068 seconds and
ran the full matrix in 353.711 seconds; see `evidence/sunbeam-pipeline.json`.
Independent rerun timings and identities are retained in `evidence/codex-*.json`. These are wall-clock measurements, not human-hours.
[CODEX_REVIEW.md](CODEX_REVIEW.md) records review coverage and limitations.
