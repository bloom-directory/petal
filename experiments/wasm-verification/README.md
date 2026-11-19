# Milestone 1: verifying shipped Wasm in a Petal

An isolated spike. Nothing here is part of the SDK, builder, CLI or release
artifacts, and no production crate depends on it.

- [SPEC.md](SPEC.md): the `split_even` benchmark, its requirements, formal
  properties and per-requirement status.
- [EVALUATION.md](EVALUATION.md): what Certora Sunbeam and KWasm actually did
  with the packaged artifact, their trusted computing bases, coverage,
  timings and the concrete no-go for the complete benchmark.
- [evidence/](evidence/): hashes, decoded target, rule and claim sources,
  verdict tables, models, residuals and logs from the recorded runs.

| Path | What |
| --- | --- |
| `split-even-petal/` | The fixture Petal, scaffolded by `petal new`, built and packaged with the ordinary CLI |
| `variants/faulty/split.rs` | The negative control, swapped in by the build script |
| `tools/wasm-evidence/` | Extractor, decoder, Sunbeam harness encoder and concrete runner (its own Cargo workspace) |
| `kwasm/` | KWasm claim generator and the one trusted lemma |
| `scripts/` | `build-and-extract.sh`, `run-sunbeam.sh`, `run-kwasm.sh`, `collect-evidence.sh` |
| `out/` | Generated and ignored: archives, components, modules, harnesses, full Prover reports |

**Outcome:** useful functional verdicts, but no complete single-backend
guarantee and no kernel-checked certificate. The custom KWasm rewrite remains
an open obligation. See [CODEX_REVIEW.md](CODEX_REVIEW.md) for independent
review and [IRIS_REFERENCE.md](IRIS_REFERENCE.md) for the Iris-Wasm prediction
(reference only).

Reproduce, with the tools pinned in [EVALUATION.md](EVALUATION.md#commands):

```sh
scripts/setup-sunbeam.sh                                      # tested macOS/arm64 bootstrap
. /tmp/petal-m1/env.sh
scripts/build-and-extract.sh
CERTORA=<dir with emv.jar> scripts/run-sunbeam.sh              # java, z3, cvc5, rustfilt on PATH
KWASM_SRC=<wasm-semantics at v0.1.157> scripts/run-kwasm.sh    # Docker
scripts/collect-evidence.sh
python3 -m unittest discover -s scripts -p 'test_verdicts.py'
python3 scripts/check-evidence.py
```

`run-kwasm.sh` leaves a container named `petal-m1-kwasm`; remove it with
`docker rm -f petal-m1-kwasm`.
