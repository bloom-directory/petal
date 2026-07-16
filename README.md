# Bloom Petal

This repository is the release authority for the Bloom Petal component
contract, Rust SDK, route builder, CLI, templates, and conformance fixtures.

The initial contract is `bloom:route@0.1.0`. Its canonical WIT is under
`wit/route/`; `bloom:sign/signing@0.1.0` is the only supported signing
interface.

## Workspace

- `bloom-petal-contract` embeds the canonical WIT and exposes contract IDs,
  capability mappings, and a deterministic WIT digest.
- `bloom-petal-sdk` is imported as the Rust crate `petal` by route components.
- `bloom-petal-builder` discovers route files and builds deterministic WebAssembly
  components.
- `bloom-petal-cli` provides `petal build`, `petal check`, `petal inspect`, and
  `petal new`.

## Development

```sh
scripts/check.sh
cargo run -p bloom-petal-cli -- inspect
```

Regenerate the committed Rust bindings after changing WIT:

```sh
cargo install --locked wit-bindgen-cli --version 0.57.1
scripts/generate-bindings.sh
```

The generated SDK bindings are derived output. `wit/route` is the only
authoritative WIT tree.

The conformance build emits the WIT digest
`2a12e23f13d4f93700a11c6af3e6996a540a4cc9c7ff0e2f9522583404095f7e`.
Consumer CI must reject a different digest until it intentionally upgrades the
contract release.

## Distribution

Rust packages are published to crates.io at coordinated exact versions. GitHub
Releases carry the WIT archive, checksums, source archive, and optional CLI
binaries. Git tags identify source provenance but are not the terminal Cargo
dependency channel.

`scripts/release-check.sh` runs the release gates and assembles the canonical
WIT archive, contract provenance, and SHA-256 checksums under `dist/`. A pushed
version tag publishes those artifacts through GitHub Releases. crates.io
publication is performed in dependency order: contract, SDK, builder, CLI.
