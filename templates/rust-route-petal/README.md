# {{petal-name}}

A Bloom Petal built on the Petal SDK v{{sdk-version}}. Before extending it, load
`.agents/skills/bloom-petal-development/SKILL.md` into your coding agent.

## Build

You need Rust and its WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
```

Components build from the route crate's tested lockfile, so run the route
tests first. They write `route/Cargo.lock`; commit it.

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
scripts/build.sh
scripts/petal.sh check
```

`scripts/petal.sh` runs the `petal` CLI of the SDK release that
`route/Cargo.toml` pins. The first time it sees a tag, it checks that the
tag names the commit in `route/Cargo.lock`. It installs that CLI, and the
`wasm-tools` release it builds with, under `~/.cache/bloom-petal`.

## Run it in Bloom

```sh
bloom petals build .
bloom petals install .
bloom vfs cat /petals/{{petal-name}}/status.json
bloom vfs cat /petals/{{petal-name}}/account/<wallet>/0/address.json
```

A packaged Bloom installs and runs this Petal. It signs only for Petals in its
installer provenance, which a developer build of Bloom can enroll; see the
skill's "Run it in Bloom".

## Upgrade the SDK

The SDK tag in `route/Cargo.toml` is the only version this Petal pins.
Change it, then refresh the lockfile and the development skill:

```sh
cargo update --manifest-path route/Cargo.toml -p bloom-petal-sdk
scripts/petal.sh skill --root .
scripts/build.sh
```

CI and the release workflow follow the lockfile.

## Release

Push a `vX.Y.Z` tag. `.github/workflows/release.yml` builds the Petal with
the SDK's CLI and publishes `{{petal-name}}-vX.Y.Z.petal.tar.gz`, its checksums
and its provenance to the GitHub release.
