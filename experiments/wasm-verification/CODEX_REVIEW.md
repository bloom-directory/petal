# Independent Codex review

The review covers the implementation, specification encodings, exact emitted
bodies, extraction/harness transformation, verifier results and relevant
Sunbeam source. It supports the Milestone 1 function-level tooling decision
under the trust boundaries in [ARGUMENT.md](ARGUMENT.md), with no
independently kernel-checked certificate.

## Guarantee and reviewed reasoning

The complete claim combines automated universal functional checks with a
manual argument about the emitted instructions. P-RET is derived from the
finite, call-free sequence and the two in-bounds stores under PRE. P-FRAME
also follows directly from those two stores. Neither property is assumed as
a precondition. The argument includes the correspondence between valid
concrete memory accesses and Sunbeam's unbounded byte-map abstraction.
The function reads no initial memory and uses no globals, tables or host
calls. This explains applicability to arbitrary valid invocation memory,
beyond the module-initialized state used by a verification harness.

The arithmetic encodings were reviewed against mathematical-integer intent:
conservation includes `first <= total`; the ordered gap includes
`first <= second`. These guards rule out modular subtraction loopholes.
The claim begins at the core-function invocation. Validity of public-route
inputs, canonical-ABI wiring and the caller's result slot are separate
obligations outside this experiment.

## Findings addressed

- A solver success alone did not establish normal return. The manual
  finite-body and model-correspondence argument is now a first-class part of
  the specification and assessment. The existential return witness remains
  explicitly distinct from a universal return proof.
- Missing/incomplete reports and inconsistent process exits could otherwise
  be mistaken for verdicts. The native-only parser fails closed, with two
  regression tests covering unknown statuses, missing reports and error or
  timeout exits. The record checker requires the expected complete matrix
  and matching input identities.
- The concrete runner's gap flag formerly clamped negative differences. It
  now rejects reversed shares and has an extreme-value regression. Its
  aggregate pass/fail was already protected by the separate ordering check.
- Archive identities were refreshed after packaged fixture documents changed.
  Both final archives reproduced exactly on an independent rebuild. The
  component/module/body/harness identities match the retained verification.
- Compiler/tool versions, source revision, checksummed native assets and
  separate dependency locks make the commands reproducible. Process errors
  from concrete runs are not swallowed. Java runs headlessly.
- Whole-module decoding is distinguished from semantic checking of reachable
  rule/target instructions. No guarantee is inferred for every function in
  the module, or for unmodelled host behavior.

The record checker binds the manual argument to the exact reviewed body
hashes and restricted instruction set. It does not itself prove semantics;
a future body change requires fresh review. A stale-hash check is not the
negative control: the faulty body is actually analyzed under its own identity.

## Independent evidence and checks

- [codex-rebuild.json](evidence/codex-rebuild.json): both variants rebuilt and
  packaged in about 20 seconds, reproducing archive/component/module/harness
  bytes exactly.
- [codex-extraction-crosscheck.json](evidence/codex-extraction-crosscheck.json):
  each component read directly from its archive and the extracted module
  compared with pinned `wasm-tools component unbundle`. Shared parser code
  limits the independence of this cross-check; it is not a parser theorem.
- [codex-sunbeam-rerun.json](evidence/codex-sunbeam-rerun.json): conservation,
  ordering and gap independently rerun on both variants with Z3 and cvc5,
  with traps asserted: 12 matching outcomes. Concrete execution confirms
  conservation failures for 1, 7 and u64::MAX on the faulty artifact.
- The Sunbeam-only pipeline was rerun after simplification: all 72 outcomes
  match expectations, with identical packaged-artifact identities; see
  [sunbeam-pipeline.json](evidence/sunbeam-pipeline.json). The complete retained
  matrix agrees across solvers and trap modes.
  The out-of-bounds return witness documents Sunbeam's model limitation.
- Workspace all-features tests, formatting and clippy passed during the
  experiment. Focused checks cover the fixture's four host regressions,
  route architecture, normal packaging, pinned-tool `petal check`, helper
  formatting/clippy, the gap-reporting regression, native verdict parsing
  and retained identities/outcomes. The release/conformance script was not
  rerun for this isolated change; production/WIT/release files are unchanged.
- The native bootstrap was smoke-tested with provisioned assets, rather than
  installed again on a clean machine. Retained model text is normalized for
  trailing whitespace; original reports remain in ignored `out/`.

## Trust and scope

Extraction/decoding/validation, body-preserving harness construction, formal
property encodings, the reviewed correspondence argument, Sunbeam's relevant
semantics/translation/optimizations and SMT solvers remain trusted. No Rust
source proof, verified compiler/extraction theorem or kernel certificate is
claimed. The negative control checks integration rather than prover soundness.

Sunbeam's finite-memory/trap gap is closed for this benchmark by the manual
argument under PRE, not by an automatic no-trap result. There is no
frame-specific faulty variant. Component wiring, initialization, JSON,
public-route behavior, host effects, fuel and availability remain outside the
claim. Future Bloom host modelling, a development fork and the next
commitment gate are deferred. Iris-Wasm remains a reference assessment only.
