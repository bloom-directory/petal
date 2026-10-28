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

Read the contract and WIT digest from `petal inspect` using the pinned tooling.
Release provenance and checksums must describe the WIT shipped with that release;
a copied digest in authoring documentation is not a compatibility oracle.

## Host error detail

Host calls return the same `SdkError::Host(status)` with or without the
`host-error-detail` feature, and `message()` is unchanged, so existing matches
keep working when the feature is enabled anywhere in the build. Classification
is the same as in earlier releases.

With the feature, `petal::sdk::last_host_reason()` returns the host's
explanation of the latest SDK host call, when that call failed with a host
message of the form `denied: <reason>`, `not found: <reason>` or
`invalid: <reason>` whose label matches its status. The reason belongs to that
call, not to the error value: every host call replaces it, and a success or an
error the SDK builds itself clears it. Read it right after the failing call.
No WIT change is required.

Detail is limited to 512 Unicode characters; control, bidi-control and zero-width characters become spaces
and a truncated explanation ends with `…`.
Hosts must supply caller-safe messages; length limits cannot redact unknown
credentials. Machine's HTTP URL redaction is tracked in
[Bloom #364](https://github.com/bloom-directory/bloom/pull/364). Adopt that host
fix before enabling richer errors in consumers that perform HTTP requests.

For the transport regression, run
`sh scripts/check-component-errors.sh "$MACHINE_CHECKOUT"` against a committed
Machine checkout. It builds the opted-in conformance component and checks two
distinct synthetic refusals through the actual Machine VM. It grants no signing authority.
The existing CI workflow exposes this check through manual dispatch with one
full `machine_ref`; ordinary CI also builds the opted-in component.

Current Machine converts a route's -2 response to message-less PermissionDenied,
so an agent using IPC still sees only "permission denied". The address helper
and conformance fixture use -2 for denials; VM-level evidence does not prove
IPC propagation. Machine's owner must confirm which exact-signing explanations
are safe to expose before a consumer routes them through another error code.

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
an existing output. Generated `.git`, `.jj`, `target`, and `artifacts` trees are
excluded; route components under the configured Petal output remain canonical.

Petal repositories should publish with the reusable workflow documented in
[`docs/releasing-petals.md`](docs/releasing-petals.md). The Petal repository
owns its release assets; Bloom only downloads and verifies them during setup.

## Distribution

Rust packages are published to crates.io at coordinated exact versions. GitHub
Releases carry the WIT archive, contract provenance, the Petal development
skill, checksums and the source archive. Git tags identify source provenance
but are not the terminal Cargo dependency channel.

`scripts/release-check.sh` runs the release gates and assembles the canonical
WIT archive, contract provenance, the Petal development skill, and SHA-256
checksums under `dist/`. Assembling them needs git 2.40 or later, for
`git archive --mtime`. A pushed version tag publishes those artifacts through
GitHub Releases. crates.io publication is performed in dependency order:
contract, SDK, builder, CLI.
