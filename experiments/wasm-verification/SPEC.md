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
- **verifier (KWasm)**: a K all-path reachability proof, by the K Haskell
  backend and z3, over the KWasm semantics, about pykwasm's decoding of the
  shipped body in a hand-written invocation configuration. Also trusted; no
  proof object is exported. KWasm cannot load the whole shipped module.
- **kernel certificate**: an independently checkable proof object (for
  example Iris-Wasm/Rocq). None exists for this benchmark.
- **source proof**: a proof about Rust source (Verus, Kani, …). None is
  claimed; it would not cover the shipped Wasm.

Status words: **verified** (by the named backend, under the stated
assumptions), **tested**, **not established** (attempted; the backend cannot
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
| SPLIT-1 | The two shares sum to the original total, for every `u64` total | `P-SUM` | Sunbeam `rule_split_1_conservation`; KWasm `props` / `props-lemma`; tests | **verified in Sunbeam's trusted functional model**; KWasm **conditional only** with custom lemma, not established without it |
| SPLIT-2 | The first share is no larger than the second | `P-ORD` | Sunbeam `rule_split_2_ordered`; KWasm functional claim; tests | **verified in Sunbeam's trusted functional model**; KWasm conditional only |
| SPLIT-3 | The shares differ by at most one | `P-GAP` | Sunbeam `rule_split_3_gap`; KWasm functional claim; tests | **verified in Sunbeam's trusted functional model**; KWasm conditional only |
| SPLIT-4 | Every valid invocation returns normally | `P-RET` | Direct KWasm `returns` claim without the custom rewrite, plus independent finite-body argument below; shipped tests | **checked in KWasm's trusted invocation model + manual finite-body argument**; not established by Sunbeam alone |
| SPLIT-5 | Only the 16-byte result slot is written | `P-FRAME` | KWasm `frame-below` / `frame-above` without custom rewrite; Sunbeam frame rule; full-memory test comparison | **verified in the named trusted models**, no frame-specific negative control |
| SPLIT-R1 | Regression: zero | `P-R0` | Sunbeam zero rule; host/shipped tests | Verified in Sunbeam's model + tested; KWasm exact result is conditional |
| SPLIT-R2 | Regression: odd totals | `P-R1` | Sunbeam rule for 7; host tests including 1, 3, 5, 7, 1 000 001, 2^64−3; shipped tests for 1 and 7 | Verified in Sunbeam's model for 7 + tested; KWasm exact result is conditional |
| SPLIT-R3 | Regression: `u64::MAX` | `P-RMAX` | Sunbeam max rule; host/shipped tests | Verified in Sunbeam's model + tested; KWasm exact result is conditional |

No row makes an unchecked assumption into proof success. **The exit decision
is a no-go for the complete guarantee with either backend alone**, not a claim
that the entire Petal is proved. Kernel certificates and source proofs: none.

**Normal return argument.** The decoded bodies have no loop, branch, call,
memory growth, division or other trapping arithmetic. Both stores access
exactly the two halves of `[p,p+16)`, which fits under PRE, including for an
unaligned `p`. Integer locals, constants, unsigned shift and subtraction are
total Wasm operations. Thus the finite body reaches its end normally under
the resource exclusions. The direct K return-state checks need no custom
arithmetic lemma. This is a manual argument about the decoded bytecode plus
a trusted-model check; a K reachability verdict alone is not a general
termination proof. The extractor, decoder and constructed invocation state
remain trusted. Any body change needs fresh review of this argument.

"With the lemma" means the conditional check uses the unproved rewrite in
[kwasm/split-even-lemmas.md](kwasm/split-even-lemmas.md) (`X >>Int 1 = X /Int 2`
for `X ≥ 0`), because K's SMT encoding treats `>>Int` as uninterpreted;
without it the same claims are not established. A `#Top` verdict using this
rewrite is classified `CONDITIONAL_UNPROVED_LEMMA`, never proof success. The KWasm claims are stated in the
mathematical form of the table above (`+Int`, `<=Int` on unbounded integers).

"Assuming return" is literal: a Sunbeam assertion after the call is checked
on the executions Sunbeam's model lets return. Because that model does not
trap on out-of-bounds stores, it lets more executions return than Wasm does,
never fewer, for this body (it has no other trapping instruction), so SPLIT-1
to SPLIT-3 and SPLIT-5 are not weakened by it. That argument rests on the
decoded instruction list, not on a backend check.

## 2. Negative control

[variants/faulty/split.rs](variants/faulty/split.rs) returns
`(first, first)`. Built and packaged by the same script, it has its own
archive, component, module and body hashes, and Sunbeam analyzes its own
harness: SPLIT-1, SPLIT-R2 and SPLIT-R3 fail with counterexamples (the
SPLIT-1 model is consistent with total 1), while SPLIT-2, SPLIT-3, SPLIT-5 and
SPLIT-R1 still hold, as they should for that fault. KWasm's claims about its
body fail for the sum and exact result (residual `TOTAL = TOTAL/2 + TOTAL/2`)
and hold for the frame. The shipped-module runs fail for totals 1, 7 and
`u64::MAX`. This tests the integration, not either verifier's soundness.

## 3. Out of scope for Milestone 1

| Item | Why |
| --- | --- |
| The public route `split/[total].json`: parameter parsing, JSON encoding, canonical ABI lift/lower, component wiring | Milestone 2 |
| Bloom host behaviour, fuel, Machine composition | Milestone 2; the target imports nothing |
| Evidence registry, signing, challenge and upgrade | Milestone 3 |
