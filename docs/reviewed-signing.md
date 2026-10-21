# Construct supported messages once

Enable the SDK's `evm-messages` feature for generic personal signing or EIP-712.
`EvmMessage` constructs the preimage, hash and `bloom.message.review.v1` evidence
together. Its completed request exposes those fields read-only; retry hints and
advisory data can be attached without rebuilding them.

In a real route, obtain the account and origin from trusted context:

```rust,ignore
let account = petal::wallet_account(ctx)?;
let origin = petal::ClaimOrigin::from_ctx(ctx)?;
let prepared = petal::evm_messages::EvmMessage::personal("Bloom example")?
    .prepare(
        &origin,
        account.wallet(),
        "example.personal",
        &petal::ClaimEffects::moves_nothing(),
        petal::ClaimNonce::Derived,
    )?;
let outcome = petal::sdk::sign_payload(prepared.request());
```

This informational text moves no value. An economic authorization needs its
explicit effects; the encoding cannot infer them. Persist an explicit nonce
when adopting an existing protocol's retry identity instead of silently changing
it to the SDK's derived convention.

For generic EIP-712, deserialize the exact domain, types, primary type and message
into `petal::evm_messages::TypedData`, then call `EvmMessage::eip712`. The published
Ether Mail hash is tested independently. Generic typed fields do not establish
a venue's economic meaning or add an installer-authorized operation class.

Protocol adapters remain Petal-owned. Polymarket's prepared records and delegated
Hyperliquid actions use PM80 profiles that reject financial claim overrides and
independently reconstruct signed facts. Choose `no_financial_overrides` explicitly
for those profiles; it does not assert zero economic effects or verify anything.
Build the existing `PetalClaim` with the protocol's exact payload and retry nonce,
then use `reviewed_sign_request` to attach one public evidence value to approval
and, when a key reference is present, delegated signing use. Empty evidence is
refused locally. Unsupported or unknown profiles need Broker implementation and
fixtures; these helpers grant no authority.

The low-level request API remains available. Mutating a cloned low-level request
can break its bindings and is outside the completed constructor's guarantees.
Broker independently rebuilds bytes and enforces host, installer and policy
checks before Signer releases a signature.

From this SDK checkout, emit a public producer fixture without keys or services:

```sh
cargo run --locked -p bloom-petal-sdk --features evm-messages --example reviewed_message
```

This demonstrates encoding only. It does not install a Petal, obtain owner
approval or prove authorization. To check the produced bytes through the actual
Broker reader, supply its source checkout and the selected signer address:

```sh
sh scripts/check-reviewed-message.sh "$BROKER_CHECKOUT" "$SIGNER_ADDRESS"
```

The command uses a fresh retained source copy and the real reader's normal
output. It compares the rebuilt preimage and hash, checks changed payload/hash
and unsupported-kind refusals, and checks changed evidence for the personal and
EIP-712 examples. It refuses unsupported review
formats, identifies the Broker commit, and reports `authorized_in_test: false`.
It does not modify the supplied checkout. An optional third argument selects an
existing public producer fixture instead of generating the supported example.
Dependency/tool provisioning may use the network; Cargo offline mode works once
those sources are available. This adapter targets the PM80 message-review v1
reader, not arbitrary host versions.

The real
PM80 installer/Machine/Broker/Signer harness supplies separate authorization
evidence; new examples still need their own component and profile acceptance.

To build the constructor as a component and check its actual Machine forwarding,
run the combined public fixture check:

```sh
sh scripts/check-component-message.sh "$MACHINE_CHECKOUT" "$BROKER_CHECKOUT" "$SIGNER_ADDRESS"
```

This builds the checked-in `personal/[wallet]/[index]/sign` and
`typed/[wallet]/[index]/sign` components, applies
Machine's normal compiled metadata and capability validation, compares two
Machine compositions and archives, captures account 1 and a stable retry through
the real WASM runner, and passes those captured bytes to the actual Broker
reader. The result generates package hash, archive digest, route IDs and proposed
class records from Machine's authoritative result. Those records are proposals;
they grant no installer authority. Removing the compiled signing capability
must fail Machine validation.

The default test-only adapter uses a synthetic account projection and a capture host
that returns pending approval. It produces no signature and cannot replace the
real installer/DaemonPetalHost/Broker/Signer authorization test. The supplied
checkouts are archived without modification; evidence stays in a fresh retained
temporary directory. The script currently checks these bounded examples, not
an arbitrary protocol.

For the stronger authorization check, set `BLOOM_INTEGRATION_MACHINE_BIN`,
`BLOOM_INTEGRATION_BROKER_BIN`, `BLOOM_INTEGRATION_SIGNER_BIN` and
`BLOOM_INTEGRATION_DEBUG_DRIVER_BIN` to the selected developer-mode service
binaries, then append `--authorize` to the same command. Machine must include
the `sdk-message` case in `scripts/test-polymarket-authorization.sh`. This mode
uses the actual installer and service boundary with a disposable public test
mnemonic and software owner. Both formats require approval, reject an
unenrolled class without an approval hint, and produce signatures recovering
the selected account-1 owner. Only a passing run reports `authorized_in_test:
true`. It submits no transaction and does not prove physical-owner acceptance.

On an installed example, write exactly `sign` to its route. The owner reads
`petal-signing-requests/` through the normal Bloom VFS, opens the pending
record's ceremony, approves, then retries `sign`. The same record reports
`signed` after completion. An unapproved retry is refused while its approval
and request identity remain unchanged. These fixed informational messages
demonstrate signing; they do not return a reusable signature to the caller.
Protocol applications must handle `SignOutcome` and their own submission.

The existing CI workflow also offers this check through
`workflow_dispatch`: supply one full `machine_ref`; its release compatibility
manifest selects Broker. This diagnostic uses exact revisions without adding
an unmerged Broker pin to SDK release inputs. Select `authorize` and supply a
full reviewed `signer_ref` for real authorization; otherwise authorization is
false. Neither mode replaces final release conformance.

The fixture also contains `stage/[wallet]/[index]/self-call`. Writing an explicit
chain name stages a zero-value, empty-data call to the selected account's own
address; an empty chain fails locally. Inspect a real host's normal
`wallets/<wallet>/<index>/chains/<chain>/outbox/pending/` for its staged request.
The route never confirms or broadcasts it; a real transaction would still
spend gas. The combined check captures the staging request and verifies account,
chain, destination and value without signing or contacting a chain.
