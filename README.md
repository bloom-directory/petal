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
Releases carry the WIT archive, checksums, source archive, and optional CLI
binaries. Git tags identify source provenance but are not the terminal Cargo
dependency channel.

`scripts/release-check.sh` runs the release gates and assembles the canonical
WIT archive, contract provenance, and SHA-256 checksums under `dist/`. A pushed
version tag publishes those artifacts through GitHub Releases. crates.io
publication is performed in dependency order: contract, SDK, builder, CLI.

### Economic effects for reusable signing

`petal::effects::Effects` builds the existing effects-v1 signing claim from the
exact ordered payloads, trusted route context and typed raw-token debits,
destinations and fees. For example:

```rust,ignore
let effects = petal::effects::Effects::new(
    "evm-137",
    petal::effects::PermissionEffect::None,
).with_debits(vec![petal::effects::Debit {
    asset: petal::effects::Asset {
        chain: "evm-137".into(),
        asset: collateral_contract.into(),
    },
    amount: gross_signed_debit.to_string(),
}]);
let claim = effects.batch_claim(ctx, operation_class, crypto_suite, &payloads)?;
```

The protocol adapter derives these facts from the prepared signing bytes. The
builder provides canonical serialization and stable payload/route bindings; it
is not a verifier or an authorization. Machine binds the real spending account,
and Broker independently checks the supported protocol and reviewed allowance.
Unverified formats and continuing permissions retain their existing approval
requirements. Use `RequiresSeparateApproval` for credential creation, token
allowances, delegation or other changes that authorize later actions.

Keep approval state, deduplication, ambiguous-signature recovery and accounting
in Machine/Broker/Signer. Petal-side hints remain advisory and must never replace
those authorities. The route WIT and signing request ABI are unchanged.

### Prepared signing contract

`bloom:prepared-sign/signing@0.1.0` is a separately versioned import; the
existing `bloom:sign/signing@0.2.0` remains available. A component using the
new import requires a host implementing it and the `bloom:sign` capability;
there is no fallback to legacy signing. The separate `bloom:prepared-sign/status@0.1.0` import requires
`bloom:sign.status` and does not confer signing capability. `sign` and read-only `status` take
UTF-8 JSON and return JSON. Their authoritative DTOs, validation, and
cryptographic suite identifiers live in `bloom_petal_contract::prepared`.
Status uses the authenticated executing route namespace.

Version 1 bounds the encoded request and response to 1 MiB, signing preimages
and review artifacts to 64 KiB each, operation class and review format to
128 bytes, operation keys to 256 bytes, and bounded declarations to 32
entries. ERC asset identities use lowercase 20-byte hex addresses, networks
use canonical `evm-<decimal chain id>`, and quantities and token IDs are
canonical decimal uint256 strings. `Unspecified` is the default and differs
from an explicit `None`. Declarations are not proof of economic effects.
The host must validate the DTO, independently recompute the suite-specific hash,
and derive identity and policy from authenticated context. For Ed25519 the
hash binds the message with SHA-256; the signature is over the message itself.
