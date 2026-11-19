# split-even

Verification fixture for Milestone 1 (see `../SPEC.md`). One read-only route,
`split/<total>.json`, returns `{"total", "first", "second"}` as decimal
strings for a canonical decimal `u64` total. Its arithmetic lives in
`route/src/split.rs`, the function the bytecode verifiers analyze.

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
```

The route crate pins this repository's SDK by path and is its own Cargo
workspace. Do not change `route/src/split.rs` without refreshing the evidence
under `../evidence/`.
