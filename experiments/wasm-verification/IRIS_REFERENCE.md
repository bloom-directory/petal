# Iris-Wasm: prediction from this experiment

Reference assessment requested during the spike. **Iris-Wasm was not
installed, integrated, or run. These are predictions, not verification results.**

The emitted correct body is small: local accesses, an i64 constant, unsigned
right shift, subtraction, two i64 stores, and return. It has no loops, calls,
host effects, floating-point instructions, or allocation. That is a favorable
target for a manual Iris-Wasm proof, even though the surrounding Rust-generated
module uses newer features and instructions unrelated to this function.

| Concern seen in this spike | Prediction for Iris-Wasm |
| --- | --- |
| Finite memory / store traps | Favorable: the program logic has both successful and failing store rules, with explicit memory resources. The result slot and memory length must be supplied in the precondition. |
| Shift versus division | Favorable in principle: prove the integer/bit-vector identity in Rocq rather than introduce an unproved simplification. It still needs an actual proof. |
| Independently checked assurance | Stronger potential: a completed theorem and its dependencies can be checked by Rocq. Audit theorem statements and assumptions; do not count `Admitted` or new axioms as a discharged obligation. |
| Normal return / termination | Separate obligation. Ordinary weakest-precondition safety/partial correctness does not itself prove termination. This body's finite instruction sequence makes a finite-step argument plausible. Include normal return, not just an invariant or a postcondition. |
| Packaged bytes to model | Still a gap until addressed. Kernel-checking a theorem about an AST does not validate an external extractor or decoder. Keep exact byte binding and invocation-state construction explicit. |
| Whole generated module / components | Uncertain. The Iris-Wasm README describes a Wasm 1.0 foundation. Modern WasmCert advertises Wasm 2.0 and extensions, but that does not automatically upgrade this program logic or establish component-model support. |
| Development effort | Likely substantially more manual proof and environment work than Sunbeam. No proof-development timing has been measured for Iris-Wasm in this experiment. |

Provisional prediction: promising for a stronger certificate for **this
function**, substantially less certain for a complete modern Rust-generated
module and component. A future experiment should require (1) a theorem about
this exact decoded body under the existing PRE, including normal return,
(2) an explicit account of how the theorem is tied to packaged bytes, and
(3) successful independent checking and an audit of the theorem's assumptions.
It would not require rewriting the Petal in Rocq or Lean.

Sources read on 19 November 2026:

- [Iris-Wasm README at e37f8387](https://github.com/logsem/iriswasm/blob/e37f8387a1171d104d78a2f77f9e637e090cf3d4/README.md): foundation, proof-rule layout, and installation requirements (at least 8 GB RAM; compilation may take over 30 minutes).
- [Memory rules at that revision](https://github.com/logsem/iriswasm/blob/e37f8387a1171d104d78a2f77f9e637e090cf3d4/theories/iris/rules/iris_rules_resources.v): `wp_store`, `wp_store_failure`, and load rules.
- [WasmCert README](https://github.com/WasmCert/WasmCert-Coq#binary-parser-experimental): current features and explicit warning that its experimental binary parser is unverified.
