# Milestone 1: verifying shipped Wasm in a Petal

This isolated experiment selects Sunbeam as the provisional verification
backend for Rust-generated Wasm in Petals. No production crate depends on it.

**Milestone 1 is achieved under explicit trust assumptions:** Sunbeam checks
the functional properties for every u64 input on the packaged function body;
a reviewed bytecode argument establishes normal return, memory safety under
the invocation precondition, and correspondence to the checked model. This is
function-level assurance, with manually reviewed obligations and no
independently kernel-checked certificate for that Sunbeam result.

A separate [WasmCert proof](wasmcert/README.md) now adds a kernel-checked
execution theorem and faulty-artifact counterexample for the same packaged
function, with explicit extraction, decoding and invocation boundaries.

- [SPEC.md](SPEC.md): stable requirements, formal properties and coverage.
- [ARGUMENT.md](ARGUMENT.md): why the complete benchmark guarantee follows,
  including normal return and the trust boundaries.
- [EVALUATION.md](EVALUATION.md): outcomes, pins, commands, effort, timings,
  Sunbeam gaps and potential Bloom alignment work.
- [CODEX_REVIEW.md](CODEX_REVIEW.md): independent review and checks.
- [evidence/](evidence/): identities, decoded functions, rules, verdicts,
  models, concrete regressions and verification timings.
- [IRIS_REFERENCE.md](IRIS_REFERENCE.md): reference-only certificate option;
  no Iris-Wasm integration.

The fixture uses the existing SDK and ordinary Petal build/package workflow.
The faulty source under `variants/faulty/` is separately packaged and analyzed
under its own artifact identity. `tools/wasm-evidence/` is a separate Cargo
workspace for extraction, harnessing and concrete tests. `out/` is generated
and ignored.

Reproduce on the tested macOS/arm64 host with the prerequisites and pins in
[EVALUATION.md](EVALUATION.md#pins-and-reproduction):

```sh
scripts/setup-sunbeam.sh
. /tmp/petal-m1/env.sh
scripts/build-and-extract.sh
scripts/run-sunbeam.sh
scripts/collect-evidence.sh
python3 -m unittest discover -s scripts -p 'test_verdicts.py'
python3 scripts/check-evidence.py
```

A development fork, host-model implementation and the next route-level
commitment gate are deferred. No runtime, sandbox, signing, registry or
lifecycle changes are part of this experiment.
