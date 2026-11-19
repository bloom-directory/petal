# split-even: specification, requirement by requirement

Milestone 1 benchmark. This file maps each requirement to the formal property
that states it exactly and to the evidence that covers it. It follows the
Conduit `SPEC.md` pattern: stable IDs, propositions kept separate from the
artifacts that check them, and an explicit status per requirement.

Evidence types are never merged:

- **test (source)**: `cargo test` of the route crate on the host. Exercises
  the Rust source, not the shipped Wasm.
- **test (shipped)**: concrete execution of the core module extracted from the
  packaged archive, in the wasmi 2.0.1 interpreter
  (`petal-wasm-evidence run`). Finitely many inputs; not a proof.
- **verifier (trusted)**: a verdict of Certora Sunbeam about the shipped
  function body (byte-identical inside a harness module). The verdict is only
  as good as Sunbeam's decoder, Wasm-to-TAC translation, memory model,
  optimizer and SMT solver, none of which emits a checkable certificate.
- **reviewed bytecode argument**: [ARGUMENT.md](ARGUMENT.md) establishes
  normal return and the memory frame, and explains the correspondence between
  valid concrete executions and Sunbeam's model. This part is manually
  reviewed, not mechanically proved.
- **kernel certificate**: an independently checkable proof object (for
  example Iris-Wasm/Rocq). None exists for this benchmark.
- **source proof**: a proof about Rust source (Verus, Kani, …). None is
  claimed; it would not cover the shipped Wasm.

Status words: **verified** (by the named backend, under the stated
assumptions), **reviewed** (manual bytecode argument), **tested**, **not established** (attempted; the backend cannot
or did not discharge it), **assumed** (outside every check).

## The target

```rust
// route/src/split.rs
#[inline(never)]
#[unsafe(export_name = "bloom_verify_split_even")]
pub extern "C" fn split_even_target(total: u64) -> Shares {   // #[repr(C)] { first: u64, second: u64 }
    let first = total / 2;
    Shares { first, second: total - first }
}
pub fn split_even(total: u64) -> (u64, u64) { /* (first, second) of the above */ }
```

The `split/[total].json` route calls `split_even`, which calls the exported
function; in the packaged component the route's core function contains a
direct `call` to the same function index (`direct_callers` in
[evidence/correct/extraction.json](evidence/correct/extraction.json)). The
export exists only so the function can be found in a symbol-stripped build.

The verification object is the function with that export in the unique matching
top-level core module of `petal/split-even/split/[total].json.wasm` inside the archive
written by `petal package`. On the C ABI for `wasm32-unknown-unknown` the
16-byte result is returned through memory, so the emitted function has type
`(param i32 i64)`: `p` (result address) and `total`. The decoded body and
all identities are in [evidence/](evidence/) and
[EVALUATION.md](EVALUATION.md#artifact-identity).

## Formal properties

Integers below are mathematical (ℤ); nothing wraps. `M` is the extracted
module, `f` the function `M` exports as `bloom_verify_split_even`.

**Invocation state.** A store holding an instance of `M` whose memory 0 has
byte length `L` (a multiple of 65536, at most 2^32 for this memory32 module), arbitrary contents `mem`, arbitrary
globals and tables, and an execution that reaches `call f` with operands
`p : i32` and `total : i64`, read as unsigned (`0 ≤ p < 2^32`,
`0 ≤ total < 2^64`).

**PRE** (memory/ABI precondition): `p + 16 ≤ L`. Nothing else: no alignment
(Wasm alignment is a hint), no assumption about other memory, globals or the
host. A valid Rust caller is intended to supply a live result slot; proving the public route supplies it is outside this spike.

After a normal return, let `mem'` be memory 0,
`first = Σ_{i<8} mem'[p+i]·256^i` and `second = Σ_{i<8} mem'[p+8+i]·256^i`.

| Property | Statement, for every invocation state satisfying PRE |
| --- | --- |
| `P-RET` | Executing `call f` terminates after finitely many steps with a normal return (no trap), with no results, and `L` unchanged. |
| `P-SUM` | `first + second = total`. |
| `P-ORD` | `first ≤ second`. |
| `P-GAP` | `0 ≤ second − first ≤ 1`. |
| `P-FRAME` | `mem'[q] = mem[q]` for every `0 ≤ q < L` with `q < p` or `q ≥ p + 16`. |
| `P-R0` | `total = 0` implies `(first, second) = (0, 0)`. |
| `P-R1` | `total = 7` implies `(3, 4)`; `total = 1` implies `(0, 1)`. |
| `P-RMAX` | `total = 2^64 − 1` implies `(2^63 − 1, 2^63)`. |

`P-SUM`, `P-ORD`, `P-GAP` and `P-FRAME` only constrain a returning execution;
`P-RET` is what excludes "correct if it happens to return". All of them
together are the requirement; none is claimed from partial correctness alone.

**Resource exclusions.** `P-RET` is about Wasm semantics, not a particular
engine: the host is assumed to supply enough resources for the complete call
(one additional frame, the finite body and administrative return steps),
without interruption. Bloom's fuel budget, epoch interruption,
native stack limits and memory growth by other code are out of scope for
Milestone 1.

**Encodings used by the Sunbeam harness** (bit-vector forms of the same
propositions; unsigned comparisons and arithmetic mod 2^64):

- `P-SUM` as `first ≤ total ∧ second = (total − first) mod 2^64`. Equivalent
  over ℤ: if `first ≤ total` the subtraction does not wrap, so the equation
  is `first + second = total`; conversely `first + second = total` with
  `second ≥ 0` gives `first ≤ total`.
- `P-GAP` as `first ≤ second ∧ (second − first) mod 2^64 ≤ 1`, which under
  `first ≤ second` is exactly `second − first ≤ 1` over ℤ.
- PRE as `p ≤ 2^32 − 16`, because Sunbeam's memory has no length. This admits
  every state PRE admits, and more.

## 1. Requirements

| # | Requirement | Formal property | Covered by | Status |
| --- | --- | --- | --- | --- |
| SPLIT-1 | The two shares sum to the original total, for every `u64` total | `P-SUM` | Sunbeam `rule_split_1_conservation`, with the model correspondence in ARGUMENT.md | Verified by trusted verifier + reviewed correspondence |
| SPLIT-2 | The first share is no larger than the second | `P-ORD` | Sunbeam `rule_split_2_ordered`, with the same correspondence | Verified by trusted verifier + reviewed correspondence |
| SPLIT-3 | The shares differ by at most one | `P-GAP` | Sunbeam `rule_split_3_gap`, with the same correspondence | Verified by trusted verifier + reviewed correspondence |
| SPLIT-4 | Every valid invocation returns normally | `P-RET` | Finite-body and store-range argument about the exact emitted instructions | Reviewed bytecode argument; not automatically discharged by Sunbeam |
| SPLIT-5 | Only the 16-byte result slot is written | `P-FRAME` | Two-store bytecode argument; Sunbeam frame rule; full-memory test comparison | Reviewed + verified in Sunbeam's model + tested; no frame-specific negative control |
| SPLIT-R1 | Regression: zero | `P-R0` | Sunbeam zero rule; host/shipped tests | Verified by trusted verifier + tested |
| SPLIT-R2 | Regression: odd totals | `P-R1` | Sunbeam rule for 7; host tests including 1, 3, 5, 7, 1 000 001, 2^64−3; shipped tests for 1 and 7 | Verified by trusted verifier for 7 + tested |
| SPLIT-R3 | Regression: `u64::MAX` | `P-RMAX` | Sunbeam max rule; host/shipped tests | Verified by trusted verifier + tested |

**Milestone 1 is achieved as a tooling decision and conditional
function-level verification result.** Sunbeam is the provisional backend.
The complete benchmark behavior follows from its universal functional
verdicts plus the reviewed normal-return, frame and model-correspondence
argument. The precondition and required properties have not been weakened.
Normal return is derived from the instructions, not assumed.

[ARGUMENT.md](ARGUMENT.md) gives the reasoning and trust boundaries in full.
It relies on the exact body bytes, faithful extraction/decoding/harnessing,
relevant verifier semantics/optimizations and solver results. These are
explicit trust assumptions. There is no source-level proof, verified
bytes-to-semantics theorem or independently kernel-checked certificate.

Sunbeam's out-of-bounds probe finds a returning witness where concrete Wasm
traps. Its memory model therefore cannot itself establish finite-memory
safety or general normal return. For this body, PRE makes both stores valid,
there are no other trapping operations or paths, and the instruction sequence
is finite. Valid concrete executions have matching results in the model;
its extra executions outside PRE do not weaken the universal result on the
allowed states. The existential witness rule is not a universal return proof.

## 2. Negative control

[variants/faulty/split.rs](variants/faulty/split.rs) returns
`(first, first)`. Built and packaged by the same script, it has its own
archive, component, module and body hashes, and Sunbeam analyzes its own
harness: SPLIT-1, SPLIT-R2 and SPLIT-R3 fail with counterexamples (the
SPLIT-1 model is consistent with total 1), while SPLIT-2, SPLIT-3, SPLIT-5 and
SPLIT-R1 still hold, as they should for that fault. The shipped-module runs
fail for totals 1, 7 and `u64::MAX`. The faulty body has the same finite,
two-store shape, with the store order reversed. This tests the integration,
not the verifier's soundness.

## 3. Out of scope for Milestone 1

| Item | Why |
| --- | --- |
| The public route `split/[total].json`: parameter parsing, JSON encoding, canonical ABI lift/lower, component wiring | Milestone 2 |
| Bloom host behaviour, fuel, Machine composition | Milestone 2; the target imports nothing |
| Evidence registry, signing, challenge and upgrade | Milestone 3 |
