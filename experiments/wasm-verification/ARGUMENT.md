# Why the benchmark guarantee follows

This is a reviewed argument about the exact emitted function identified in
[SPEC.md](SPEC.md), combined with Sunbeam's universal functional checks. It
is not a machine-checked termination theorem or a kernel-checked certificate.
The required properties and invocation precondition are unchanged.

## 1. Identify the concrete function

The ordinary Petal packaging workflow emits the component. Extraction reads
that component from the archive and selects the core module exporting
`bloom_verify_split_even`, function 67, type `(param i32 i64)` with no direct
results. The two u64 results occupy memory at `p` and `p+8`.

The recorded correct body is 26 bytes with SHA-256
`c9c2e6618c6f89996bf63ba97fbd617af3e66eb91c63f10401e42ca13d4beba5`.
The Sunbeam harness contains those same body bytes and the same function type.
The decoded body uses only locals, an i64 constant, unsigned shift,
subtraction, two i64 stores and its end instruction. It has no calls or
accesses to globals or tables. Its only memory accesses are the two stores.

This step trusts archive extraction, Wasm decoding/validation and the harness
construction/equality checks. Cross-checking with `wasm-tools component
unbundle` supports identity; it is not a verified extraction theorem.

## 2. Establish normal return and the memory frame

For every valid invocation, interpret `p` and `total` as unsigned integers.
PRE is `p + 16 <= L`, where memory 0 has length `L <= 2^32`.

- The first eight-byte store accesses `[p,p+8)`.
- The second accesses `[p+8,p+16)`.
- Both ranges are inside memory under PRE. Unaligned scalar stores are
  permitted by Wasm; their alignment immediate is a hint.
- The integer/local instructions are defined for every input bit pattern.
  In particular, unsigned shift and wrapping subtraction do not trap.
- The validated body has no loop, branch, call, memory growth or other
  instruction that could introduce divergence or another trap.

Consequently, the finite instruction sequence finishes normally, assuming
resources for the call and administrative return steps and no external
interruption. It does not change memory length and cannot write outside the
16-byte result slot. This establishes P-RET and P-FRAME directly for every
initial memory content, without assuming that the function returns.

This argument uses ordinary Wasm execution semantics and the exact decoded
sequence. The record checker binds the manual review to the two known body
hashes. A changed body requires fresh review; a hash check alone does not
establish semantics.

## 3. Relate valid executions to Sunbeam's memory model

Sunbeam uses a byte-map abstraction that omits finite-memory bounds traps.
For this body, restrict attention to the states admitted by PRE:

1. PRE implies `p <= 2^32 - 16`, satisfying the harness's pointer constraint.
   Neither `p+8` nor either accessed range wraps around the address space.
2. The two stores are valid in concrete Wasm. Both models therefore proceed
   through the same local/integer/store sequence and normal return.
3. Assuming faithful modelling of these specific instructions, the eight
   result bytes at each offset agree with the concrete little-endian stores.
4. The function reads no initial memory. Its returned shares are independent
   of any difference between module-initialized memory and arbitrary valid
   invocation memory. The frame property also follows separately in step 2.

Thus every valid concrete invocation has matching observable results in the
Sunbeam execution being checked. Extra returning executions outside PRE in
Sunbeam's model do not weaken the universal functional result for this body.
They do make its out-of-bounds reachability witnesses unsuitable as concrete
normal-return evidence.

This is a reviewed, trusted model-correspondence argument, not a mechanically
proved simulation theorem. It is specific to this function and PRE.

## 4. Apply the universal functional verdicts

Sunbeam checks all u64 totals and all pointers satisfying its constraint.
Both Z3 and cvc5 accept conservation, ordering and gap for the correct body.
The harness encodings avoid modular-arithmetic loopholes:

- Conservation: `first <= total` and `second == total - first` using
  unsigned comparison. The guard makes the subtraction mathematical, so
  `first + second = total` over integers.
- Ordered gap: `first <= second` and unsigned `second - first <= 1`.
  The guard excludes underflow and establishes `0 <= second - first <= 1`.

By step 3, these statements hold for the concrete results. By step 2, every
valid invocation returns normally and preserves the memory frame. Together
they establish the complete benchmark specification under the stated trust
and resource assumptions. No return assumption or desired arithmetic
postcondition is introduced as an axiom.

## Trust and scope

Trusted: extraction/decoding/validation, body-preserving harness construction,
specification encoding, the correspondence argument, Sunbeam's relevant
instruction semantics and optimizations, VC generation, and the selected SMT
solver. These are explicit trust boundaries rather than independently
kernel-checked components. No Rust-source proof or verified compiler is used
as a substitute for checking the emitted body.

The guarantee begins at a valid core-function invocation. It does not prove
that the public route constructs that invocation correctly, nor cover module
initialization, component/canonical-ABI wiring, JSON, host effects, Bloom
fuel budgets or runtime availability. Iris-Wasm remains a reference option
for stronger certificates, with no integration in this experiment.
