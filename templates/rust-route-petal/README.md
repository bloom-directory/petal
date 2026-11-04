# {{petal-name}}

Components build from the route crate's tested lockfile. Run the route tests
once, which writes `route/Cargo.lock`, and commit it; then build this Petal
with `petal build` and validate the resulting package with
`bloom petals build .`:

```sh
cargo test --manifest-path route/Cargo.toml
petal build
```

Before pushing, enforce the route architecture rules from the
`bloom-petal-development` skill:

```sh
bash scripts/check-route-architecture.sh
petal check
```
