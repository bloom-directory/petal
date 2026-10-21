# Bloom Petal

This repository is the release authority for the Bloom Petal component
contract, Rust SDK, route builder, CLI, templates, and conformance fixtures.

The route contract is `bloom:route@0.1.0`. Its canonical WIT is under
`wit/route/`; custody-aware Petals use `bloom:key/derive@0.1.0` and
payload-bearing `bloom:sign/signing@0.2.0`, including atomic ordered batches.

## Workspace

- `bloom-petal-contract` embeds the canonical WIT and exposes contract IDs,
  capability mappings, and a deterministic WIT digest.
- `bloom-petal-sdk` is imported as the Rust crate `petal` by route components.
- `bloom-petal-builder` discovers route files and builds deterministic WebAssembly
  components.
- `bloom-petal-cli` provides `petal build`, `petal check`, `petal package`,
  `petal inspect`, and `petal new`.

## Development

With Rust, the `wasm32-unknown-unknown` target, and jq installed:

```sh
scripts/install-tools.sh
scripts/check.sh
cargo run -p bloom-petal-cli -- inspect
```

The installer reads the `wasm-tools` pin from `[workspace.metadata.tools]` in
`Cargo.toml` and matches `wit-bindgen-cli` to the workspace `wit-bindgen`
dependency. It installs with `--locked` and verifies the executables on `PATH`.
Use `scripts/install-tools.sh --wasm-tools-only` when bindings are not needed.
Update the `wasm-tools` pin manually when upgrading it; Dependabot does not
track workspace metadata.

Regenerate the committed Rust bindings after changing WIT or `wit-bindgen`:

```sh
scripts/install-tools.sh
scripts/generate-bindings.sh
```

The generated SDK bindings are derived output. `wit/route` is the only
authoritative WIT tree.

The conformance build emits the WIT digest
`b1448484d252a4b6cae350df1d28e5102109692acdcd4be9b03bffe097af04f6`.
Consumer CI must reject a different digest until it intentionally upgrades the
contract release.

## Pinning the SDK

A Petal pins the SDK in the route crate's Cargo dependencies, including renamed
or workspace-inherited dependencies. Generated components use that same source
and feature selection. `[sdk]` in `petal-build.toml` is optional transitional
validation: remove it, or keep it equal to the route dependency.

The route SDK must be an exact pin: a git `rev` that is a full 40-character commit,
or a version requirement such as `=0.1.0`. `petal build`, `petal check` and
`petal package` then refuse a Petal when:

- any SDK dependency of the route crate, in any dependency table or inherited
  from its workspace, differs from `[sdk]`;
- for a git pin, another crate from the SDK's repository, such as
  `bloom-petal-contract`, is pinned at a different revision, in the route crate
  or in `[dependencies]` of `petal-build.toml`;
- the lockfile resolves any of them from another source or revision;
- the route crate or its workspace overrides the SDK with `[patch]` or
  `[replace]`, which generated route components ignore.

The component build starts from the route workspace's committed lockfile and
resolves wrapper packages without accepting dependency graph changes. Missing
locked sources may be fetched; provision them first to use Cargo's offline mode.
Declare any additional wrapper dependencies in the route workspace so route
tests and generated components exercise the same graph.

To upgrade, change the route SDK pin (and any remaining transitional pins), then refresh the
lockfile:

```sh
cargo update --manifest-path route/Cargo.toml -p bloom-petal-sdk
```

A local checkout can still be tested through a `[patch]` in
`.cargo/config.toml`, as this repository's consumer CI does. Pins outside
Cargo are not checked: scripts that install the `petal` CLI, and the release
workflow's `uses:` reference and `petal-tooling-ref`. Keep them at the same
commit.

## Packaging a Petal

After building route components, create the platform-neutral archive consumed
by Bloom:

```sh
petal package \
  --root . \
  --out dist/example-v0.1.0.petal.tar.gz
```

`petal package` checks every built component before writing the archive. The
archive uses sorted strict ustar entries, normalized ownership, permissions and
timestamps, and a gzip header with a zero timestamp. Repeating the command over
the same clean package tree produces identical bytes. It refuses to overwrite
an existing output. It includes the complete `petal/` runtime tree, `petal.toml`,
`README.md` and `AGENTS.md`. The configured output must be `petal/<name>`.
Unrelated development files do not change runtime identity; code, capabilities
and runtime documents do. This deliberately changes older source-tree package
hashes: publish a new immutable package and preserve its predecessor lineage.
Machine assigns a separate final identity when composing the installed package.

Component builds remap checkout and Cargo cache paths and omit symbol names at
compile time. Two different checkout directories with identical compiler and
locked inputs produce the same tested components. Explicit target-specific
Cargo rustflags keep their normal precedence; include equivalent path remapping
if you replace the default flags.

Petal repositories should publish with the reusable workflow documented in
[`docs/releasing-petals.md`](docs/releasing-petals.md). The Petal repository
owns its release assets; Bloom only downloads and verifies them during setup.

## Distribution

Rust packages are published to crates.io at coordinated exact versions. GitHub
Releases carry the WIT archive, checksums, source archive, and optional CLI
binaries. Git tags identify source provenance but are not the terminal Cargo
dependency channel.

`scripts/release-check.sh` runs the release gates and assembles the canonical
WIT archive, contract provenance, and SHA-256 checksums under `dist/`. A pushed
version tag publishes those artifacts through GitHub Releases. crates.io
publication is performed in dependency order: contract, SDK, builder, CLI.
