# Loops and I/O: agent-written Rocq proofs beyond straight-line code

This builds on the WasmCert pipeline in `../` (PR #58). The split-even proof
there covers one straight-line exported function. These two experiments ask
whether the same pipeline, with **proofs written by an agent**, reaches the
things arbitrary Petals need:

1. **Loops.** An exported function in a real packaged Petal component that
   iterates over linear memory.
2. **I/O, with the whole program proved.** A stdin→stdout Petal, as the
   whitepaper describes Petals: `wasm32-wasip1`, with `_start` proved
   end-to-end against a WASI `fd_read`/`fd_write` model. There is no export
   shim; the theorem is about the real entry point.

Both proofs were written by a Claude Opus 5.5 agent from a short brief.
Each passes `coqchk` with axioms identical to the `../` allowlist; both
checks were re-run independently of the agent.

## Results

| | Exp 1: `byte_sum` loop | Exp 2: upper-case filter `_start` |
|---|---|---|
| Artifact | 80,323-byte core module from the packaged Petal component; func 67, 166-byte body | whole 517-byte `wasm32-wasip1` module |
| Control flow | 4× unrolled loop plus a remainder loop (rustc `-O3`) | 3 nested loops: read, an unrolled upper-casing loop, and a write-retry loop for partial writes; 2 host calls |
| Agent wall-clock | **24 min** (20:50→21:14 UTC) | **60 min** (20:50→21:50 UTC; one context reset) |
| Compile/fix iterations | ~30 | ~65 |
| Hand-written Rocq | 1,342 lines | 2,392 lines |
| `rocqc` (all units) | 5.5 s | 5.3 s |
| `coqchk` | 26.4 s | 26.4 s |
| End-to-end `check.sh` | 34 s | 35 s |
| Axioms | = `allowed-axioms.txt` | = `allowed-axioms.txt` |
| `Admitted`/`admit`/`Axiom` | none | none |

For comparison, Gavin's split-even proof takes 35 s end-to-end on the same
native toolchain, and 69.5 s in his amd64-emulated Docker run. Almost all of
`coqchk`'s fixed ~26 s goes to re-checking the WasmCert and CompCert
libraries. The per-Petal proof itself compiles in a few seconds.

Per-unit timings are in `evidence/*-timings.tsv` and artifact hashes in
`evidence/identities.json`.

## What is proved

### Experiment 1: `packaged_byte_sum` (`rocq/ByteSumExecution.v`)

`byte_sum_selected_code`, the function body sliced from the hashed module
bytes and decoded with `vm_compute`, is `Some code`, and `code` satisfies the
following. For any store and frame where function index 67 resolves to that
`FC_func_native (Tf [i32; i32] [i64])` closure, memory 0 is `m`, and
`p + len <= mem_length m`:

```
reduce_trans (hs, s, f, [p; len; call 67]) (hs, s, f, [i64.const r])
  /\ Int64.unsigned r = mem_sum m p len      (* exact, in Z *)
```

`mem_sum_no_overflow` shows the sum is at most `255·len < 2^64`, so the
result is exact rather than "correct mod 2^64". The store and frame are
unchanged.

There is one extra hypothesis: `p + len <= 2^32`. Any real wasm32 memory
already satisfies it given the bound above. It is needed only because
WasmCert's `meminst` length is unbounded.

### Experiment 2: `packaged_upper_start` (`rocq/UpperExecution.v`)

`run_parse_module upper_module = Some md` decodes the whole module. In `md`:
- `_start` is exported as func 2;
- there are exactly 2 imports;
- `mod_funcs md = [start_code]`.

Then, for **every stdin** `inp` and **every short-read/short-write schedule**
`rs`, `ws`, in any configuration where:
- funcs 0 and 1 are the WASI `fd_read`/`fd_write` host functions, and func 2
  is `start_code`;
- global 0 (the shadow stack pointer) holds 1048576;
- memory 0 has at least 1048832 bytes;

we have:

```
reduce_trans (⟨stdin = inp, stdout = []⟩, s, f, [call 2]) (hs', s', f, [])
  /\ stdout hs' = map ascii_upper inp
  /\ stdin hs' = []
  /\ global 0 is restored to 1048576
```

That is, `_start` terminates without trapping, consumes all of stdin, and has
written exactly the upper-cased input, whatever chunking the host chooses.
The more general helper `start_correct` gives
`stdout' = stdout ++ map ascii_upper stdin` from any starting host state.

The execution proof goes through a small big-step judgement and
weakest-precondition layer (`UpperSem.v`, `UpperWp.v`). Both are proved
sound against WasmCert's `reduce_trans`, so they add nothing to the trusted
base. The agent built that layer itself, unprompted, to manage nested
`lholed` contexts. It is reusable for other Petals.

## What is trusted

The trusted base is the same as `../`: WasmCert's semantics and decoder,
Rocq's kernel (`coqchk`), the 9 allowlisted axioms, and the hash binding in
`rocq/generate.py`. Experiment 2 adds two things:

- **`rocq/WasiHost.v`, the WASI model.** It is a concrete instance of
  WasmCert's `host` class with its obligations proved. Its host state is
  `stdin`, `stdout`, `rsched`, `wsched` and call counters.
  - `fd_read(0, iov, 1, nread)` copies `min(max 1 (rsched i), iov.len,
    |stdin|)` bytes of stdin into memory at `iov.buf` and stores the count
    at `nread`.
  - `fd_write(1, iov, 1, nwritten)` *loads* `min(max 1 (wsched j), iov.len)`
    bytes from memory and appends them to stdout.
  - Any other fd or iovec count returns EBADF (8). Out-of-bounds memory
    returns EFAULT (21).

  The model is deliberately minimal. The output really is read out of Wasm
  memory, so the theorem cannot be met by a host that ignores the program.
  It is not a full WASI preview-1 semantics, and it would need review before
  becoming shared infrastructure.
- **Instantiation is assumed, not proved.** The theorem takes the
  instantiated configuration as premises: import resolution, initial global
  and memory size. WasmCert has an instantiation semantics, so proving this
  is a follow-up.

## Petal shape

The Petals in this repo are component-model routes (`bloom:route`), while the
whitepaper describes Petals as stdin/stdout programs. Experiment 1 uses the
component form, extracting a core function from the packaged component as
`../` does. Experiment 2 uses the whitepaper form directly.

Experiment 2 is a `no_std`/`no_main` Rust program that defines `_start`
itself (`wasi-petal/src/main.rs`), which produces 517 bytes. For size
comparison only, `wasi-petal-std/` is the same filter written with `std`:

| | no_std | std |
|---|---|---|
| Module | 517 B | 33,367 B |
| Functions | 1 (+2 imports) | 111 |
| Code section | 381 B | 29,629 B |
| Imports | `fd_read`, `fd_write` | + `environ_get`, `environ_sizes_get`, `proc_exit` |
| `call_indirect` | no | yes |

Under Node's `node:wasi` (`scripts/run-upper.mjs`) the module
produces the expected output, including on 100 KB of random bytes.

## Extrapolation

We have three data points for agent-written proofs on this pipeline:
split-even (straight-line, under a day including the pipeline itself), a
loop (24 min), and a whole I/O program with nested loops (60 min).

- **Proof effort tracks control-flow structure, not module size.** Each loop
  needs an invariant, and each host call needs one host-spec step. The
  agent's output rate was steady at about 40–55 lines/min.
- **Machine-check time is flat**: about 26 s of fixed `coqchk` plus a few
  seconds per Petal proof. That is negligible for CI.
- **What doesn't scale yet is code we didn't write.** The `std` version
  pulls in an allocator, formatting, panic machinery and `call_indirect`.
  Proving it this way would mean proving ~110 functions of library code. The
  practical path is the reusable layer started here: `UpperSem`/`UpperWp`,
  plus reusable specs for common runtime pieces such as `memcpy`/`memset`,
  bump allocation and the WASI calls. Then each Petal's proof covers only its
  own logic. Alternatively, Petals that want proofs build `no_std`, as here.
- **Rough expectation:** a small-to-medium Petal with hand-written logic of a
  few hundred lines of Rust and no heavy `std` dependencies should take hours
  of agent time, not weeks. The open risks are proofs that
  stall, which neither experiment hit; std-heavy code; and the amount of
  WASI the model covers.

## Reproduce

```bash
# one-time toolchain (native; ~20 min): opam switch + WasmCert 58a3d3f8
bash scripts/setup-native.sh
cat > ~/.petal-rocq/env.sh <<'EOF'
export OPAMROOT=$HOME/.petal-rocq/opam
eval "$(opam env --root=$OPAMROOT --switch=petal --set-root --set-switch)"
export WASMCERT=$HOME/.petal-rocq/WasmCert-Coq
EOF
. ~/.petal-rocq/env.sh

# artifacts (Rust 1.97.0)
RUSTUP_TOOLCHAIN=1.97.0 sh scripts/build-byte-sum.sh
(cd wasi-petal && RUSTUP_TOOLCHAIN=1.97.0 cargo build --release --target wasm32-wasip1)
echo 'hello, petal!' | node scripts/run-upper.mjs

# generate byte/hash bindings, then compile + axiom diff + coqchk
python3 rocq/generate.py
bash rocq/check.sh byte-sum   # -> out/rocq/byte-sum/status.txt = PASS
bash rocq/check.sh upper      # -> out/rocq/upper/status.txt    = PASS
```
