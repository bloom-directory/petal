# Independent Codex review

19 November 2026. First implementation pass: Claude Code 2.1.322 with API
model `claude-opus-5-5` (Claude Opus 5.5), confirmed in its completed session
metadata. No model substitution. Codex independently read the implementation,
properties, verifier output and relevant backend source/documentation, then
made the corrections below. This review is not a kernel-checked certificate.

## Findings addressed

1. **An unproved rewrite was labeled proof success.** The first KWasm table
   used `PROVED` for `#Top` results that imported the custom shift rewrite.
   The runner now reports `CONDITIONAL_UNPROVED_LEMMA`, and SPEC/EVALUATION
   state the open obligation. The exit decision is a no-go for the complete
   guarantee with either backend alone. No unchecked rewrite is counted as
   a discharged functional proof.
2. **Verdict parsing needed to fail closed.** Sunbeam output is now parsed
   as JSON and checked against the process exit status. Missing reports,
   unknown statuses, timeouts and inconsistent exits are inconclusive.
   KWasm requires a clean `#Top` and exit 0 for a successful model check;
   residuals and frontend errors are `NOT_ESTABLISHED`, not automatically
   counterexamples. Four regression tests exercise these reporting hazards.
   `check-evidence.py` rejects incomplete/unexpected matrices and inconsistent
   artifact identities. These scripts check this spike's records, not Wasm
   semantics or production evidence acceptance.
3. **Normal return needed an explicit check and argument.** Added `returns`
   claims for both exact emitted bodies, without the custom rewrite, and
   independently ran them. Clarified that K reachability is not a general
   termination checker. The decoded bodies are finite and call-free; their
   only potentially trapping operations are the two stores, which fit under
   PRE. SPEC documents the finite-body argument, invocation/ABI conditions
   and resource exclusions. The target's decoded instruction set is guarded
   by the record checker; future body changes still require fresh review.
4. **Archive records were stale after fixture documentation changed.**
   Rebuilt and packaged the final fixture and faulty variant, recollected
   evidence, and checked every K claim's archive/component/module/body
   identities against extraction. Codex's independent rebuild reproduced
   both final archives exactly. Component, module, body and Sunbeam harness
   hashes remained unchanged by the documentation edits.
5. **Reproduction pins and runtime behavior needed tightening.** Added a
   checksummed native bootstrap, enforced Rust/wasm-tools versions, pinned
   the Docker manifest digest and Python runtime dependencies, and checked
   reused-container source/image/mount/dependency identity. Removed swallowed
   concrete-run process errors. Java is headless, including child processes,
   as requested. Fixed executable permissions on the collection script.
6. **Claims exceeded demonstrated coverage in places.** Whole-module
   decoding by Sunbeam is distinguished from semantic checking of reachable
   rule/target instructions. The math gap property includes a nonnegative
   difference, matching its guarded encoding. Rust caller validity is an
   intended route obligation, not a proved public-route guarantee. Iris-Wasm
   is a source-grounded prediction only, with no third integration. The
   concrete runner's gap flag formerly clamped negative differences; it now
   rejects reversed shares and has an extreme-value regression. The combined
   pass/fail was already protected by the separate ordering check.

## Independent evidence and checks

- Rebuilt/package-tested both variants using the reviewed build script:
  [codex-rebuild.json](evidence/codex-rebuild.json), about 20 seconds,
  identical archive/component/module/harness bytes before and after.
- Read each component directly from its final tar archive and compared the
  helper's module bytes against pinned `wasm-tools component unbundle`:
  [codex-extraction-crosscheck.json](evidence/codex-extraction-crosscheck.json).
  The CLI and helper share parser code; this is a cross-check, not an
  independent parser theorem.
- Independently reran conservation, ordering and gap on both variants with
  Z3 and cvc5, with traps asserted: 12 matching outcomes, recorded in
  [codex-sunbeam-rerun.json](evidence/codex-sunbeam-rerun.json). The faulty
  artifact violates conservation under its own identity. Concrete execution
  confirms `7 -> (3,3)` and failures for 1 and u64::MAX.
- Independently checked direct K normal-return states without the custom
  lemma, then ran the reviewed complete 12-claim pipeline from a fixed
  snapshot: [codex-kwasm-returns.json](evidence/codex-kwasm-returns.json) and
  [codex-kwasm-pipeline.json](evidence/codex-kwasm-pipeline.json). The final
  pipeline exited 0 in about five minutes, with conditional results kept
  conditional. One earlier live runner edit caused a trailing shell read
  error; that attempt is not the claimed clean pipeline run.
- Workspace all-features tests, formatting and clippy passed. Fixture host
  regressions (four tests), route-architecture check, fixture/helper formatting
  and clippy, ordinary packaging and pinned-tool `petal check` passed.
  Four reporting regression tests, the concrete-gap reporting regression and
  the retained-evidence check passed. The
  entire release/conformance script was not rerun: no production/WIT/release
  files changed. The native bootstrap was smoke-tested with cached assets,
  not installed again on a clean machine.

## What remains trusted or open

Extraction/decoding, Sunbeam harness transformation, manually authored rule
and K-state encodings, each backend's semantics/translation/optimizations,
upstream simplifications and SMT solvers remain trusted. No verified
bytes-to-AST/state theorem, general compiler theorem, source proof, or
independently kernel-checked certificate was produced.

Sunbeam omits finite-memory traps. KWasm rejects an instruction elsewhere in
the full module and needs a proved arithmetic lemma for its complete
functional result. There is no frame-specific faulty variant. Component
wiring, canonical ABI, JSON, host effects, initialization, Bloom fuel and
runtime availability are not proved. No Milestone 2/3 work is implied by
the function-level evidence. The Iris-Wasm prediction has not been tested.
