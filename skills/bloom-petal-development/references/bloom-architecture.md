# Bloom Monorepo Architecture for Petal Migrations

Reference for extracting native bloom functionality into standalone petals.

## Bloom monorepo layout (`bloom-directory/bloom`)

```
bloom/
├── Cargo.toml              # workspace, members under crates/
├── crates/
│   ├── bloom/              # main daemon binary
│   ├── bloom-daemon/       # daemon wiring — registers handlers, config
│   ├── bloom-vfs/          # VFS layer — handlers/ contains native handlers
│   ├── bloom-proto/        # config types, intent types, chain/token constants
│   ├── bloom-tx/           # tx engine — stage/confirm/broadcast pipeline
│   ├── bloom-evm/          # chain client, chain registry
│   ├── bloom-keystore/     # wallet key management
│   ├── bloom-petals/       # petal runtime — WASM host, router, policy
│   ├── bloom-defi/         # EXAMPLE: native crate being migrated
│   ├── bloom-hyperliquid/  # already migrated to bloom-petal-hyperliquid
│   ├── bloom-ens/
│   ├── bloom-prices/
│   ├── bloom-etherscan/
│   └── ...
├── tests/docker/           # integration test profiles
└── docs/                   # specs, examples, audit docs
```

## Native crate + VFS handler pattern (what gets migrated)

A native feature typically spans two crates:

1. **Domain crate** (`crates/bloom-<name>/src/lib.rs`) — the API client,
   types, and business logic. E.g. `bloom-defi` has `EnsoClient`,
   `RouteRequest`, `RouteResponse`, natural-intent parser.

2. **VFS handler** (`crates/bloom-vfs/src/handlers/<name>.rs`) — maps the
   domain logic to virtual file paths (read/write). This is the layer that
   becomes petal route files.

### VFS handler structure

Implements the `Handler` trait (`bloom_vfs::handler::{Entry, Handler, HandlerError}`).
Key elements:

- `lookup` / `list` / `read` / `write` methods dispatching on path segments.
- In-memory state (`Arc<RwLock<HashMap<...>>>`) that becomes petal store.
- Side-effecting writes that stage transactions through `TxEngine` — these
  map to `bloom:tx/outbox` in the petal.
- External API calls (HTTP) — these map to `bloom:http/fetch`.
- Chain reads — map to `bloom:chain/read`.
- VFS reads (wallet data, address book) — map to `bloom:vfs/readwrite`.

## Integration points to disconnect (checklist)

When removing a native crate from bloom, every one of these must be addressed.
Missing any causes compile failures or stale runtime behavior.

1. **Workspace `Cargo.toml`**: remove from `members` list AND
   `[workspace.dependencies]` table (two separate lines).
2. **Dependent crates' `Cargo.toml`**: remove the `bloom-<name>.workspace = true`
   dependency line. Use a content search to find ALL crates that depend on it —
   in the defi/enso cutover this was `bloom-daemon`, `bloom-vfs`, and `bloom`
   (the CLI binary crate), not just the daemon.
3. **`bloom-vfs/src/handlers/mod.rs`**: remove `pub mod <name>;` and
   `pub use <name>::<Name>Handler;`.
4. **`bloom-vfs/src/handlers/<name>.rs`**: delete the handler file.
5. **`crates/bloom-<name>/`**: delete the entire crate directory.
6. **`bloom-daemon/src/lib.rs`**: remove the handler import (both the crate
   import line `use bloom_<name>::...` and the handler struct in the
   `use bloom_vfs::handlers::{...}` block), plus the entire mount block
   (`if let Some(cfg) = ... { vfs_builder.mount("<name>", ...) }`).
   Also check for debug/status log lines that reference the feature flag
   (e.g. `enso = config.enso.is_some()` → update or remove).
7. **`bloom/src/main.rs`**: search for ceremony enrichment functions that
   reference the handler's session data. For defi, this was
   `find_defi_review_for_outbox()` (called from `outbox_confirm_unlock_intent`)
   and the `DefiReview` struct — both non-test production code that enriched
   the signing ceremony with route/policy details. These need their defi-
   specific calls removed and the function simplified to use only the generic
   outbox `plan.md`.
8. **`bloom-daemon/src/ipc.rs`**: search for test-only (`#[cfg(test)]`)
   duplicates of ceremony enrichment functions and any test functions that
   construct defi session files on disk. These must be removed or they break
   the test build.
9. **`bloom-auth-api/src/lib.rs`**: may contain petal identity constants
   (`PETAL_ID_<NAME>`, `PLACEHOLDER_DIGEST_<NAME>`, signing intent strings).
   These can often stay as identity/attestation plumbing, but verify they
   don't reference the removed crate's types.
10. **`bloom-proto/src/config.rs`**: may have a config struct (e.g.
    `EnsoConfig`) that can stay for the `[<name>]` config block or be moved
    to the petal's runtime config.
11. **Docs**: update README, DEVELOPMENT.md, AUDIT.md, QUICKSTART.md,
    EXAMPLES.md references.
12. **Docker tests**: remove or update test profiles in `tests/docker/`.
13. **Comment/doc reference cleanup**: after functional removal, run a broad
    search for the old crate name in comments, doc strings, and error messages.
    Update references from `bloom-<name>` to point to the petal instead. Common
    locations: module-level doc comments (`tokens.rs`, `intent.rs` in
    `bloom-proto`), test-infrastructure comments (`bloom-evm/src/lib.rs`), and
    error messages in the tx engine. Error messages on intentionally-retained
    rejection paths (e.g. `RawIntentBody::Enso` rejected in `tx_engine.rs`
    stage) should be updated to say "petal" not "bloom-<name>" but the rejection
    logic stays — it's correct (those intents route through the petal's HTTP
    path, not native tx staging).

### What stays after cutover

Not all references to the old feature should be removed:

- **Proto-level intent variants** (e.g. `RawIntentBody::Enso` in
  `bloom-proto/src/intent.rs`) are **classification types**, not
  implementation. They stay in `bloom-proto` so the tx engine can correctly
  route (reject) them from native paths. Remove the implementation crate, not
  the classification type.
- **Config structs** (e.g. `EnsoConfig` in `bloom-proto/src/config.rs`) may
  stay as a config-block type if the daemon still reads `[enso]` from config
  to pass to the petal runtime.
- **Auth constants** (`PETAL_ID_*`, signing intent strings in
  `bloom-auth-api`) are identity/attestation plumbing — they stay unless they
  import the removed crate's types.
- **Debug/status flags** that were conditional on config (e.g.
  `enso = config.enso.is_some()`) become always-true after cutover
  (`enso = true`) since the petal is always available via PetalRouter.

### Search strategy

Run a broad content search across all `.rs`, `.toml`, and `.md` files:

```sh
grep -rn "bloom-defi\|bloom_defi\|DefiHandler\|defi_handler\|find_defi_review\|DefiReview" \
  --include="*.rs" --include="*.toml" --include="*.md"
```

Use `execute_code` with `search_files` for a faster, paginated sweep across
the entire workspace — it catches references in test modules and comments
that manual grep from a single directory misses.

After the functional removal is committed, run a second sweep specifically
for stale crate-name references in comments and docs. Update these to point
to the petal. The compile will pass with stale comments (they're just text),
so this step requires explicit attention — it won't be caught by `cargo check`.

### Compile validation

After all edits, run from the bloom workspace root:
```sh
cargo check 2>&1 | tail -30
cargo test 2>&1 | tail -30
```
Expect test failures only in test modules that directly referenced the removed
handler — fix by removing the test or adapting it to the petal's VFS surface.

## Petal repo ecosystem

| Repo | Petal | Status |
| --- | --- | --- |
| `bloom-directory/petal` | SDK, CLI, builder, contract, template | Release authority |
| `bloom-directory/bloom-petal-polymarket` | Polymarket trading | Most complete reference |
| `bloom-directory/bloom-petal-hyperliquid` | Hyperliquid perp trading | Migrated from `bloom-hyperliquid` |
| `bloom-directory/bloom-petal-near` | NEAR protocol | Migrated |

## Naming convention

- **Repo**: `bloom-petal-<service-name>` (e.g. `bloom-petal-enso`, not
  `bloom-petal-defi`). Named after the external API service, not the
  function domain. This avoids ambiguity when multiple services serve the
  same domain (e.g. enso vs 1inch for DeFi routing).
- **Petal package name**: matches the repo suffix (e.g. `enso`).
- **VFS mount path**: can differ from the petal name (e.g. `defi/` mount
  for the `enso` petal).

## Petal SDK pinning

Each petal pins the SDK via git rev in `petal-build.toml` and `route/Cargo.toml`:
```toml
[sdk]
package = "bloom-petal-sdk"
git = "https://github.com/bloom-directory/petal"
rev = "<commit-sha>"
```
Check `petal/Cargo.toml` for the current contract version and WIT digest.

## WIT capability interfaces

| Interface | Capability | Purpose |
| --- | --- | --- |
| `bloom:http/fetch` | `bloom:http` | Outbound HTTP requests |
| `bloom:store/kv` | `bloom:store` | Persistent key-value storage |
| `bloom:sign/signing` | `bloom:sign` | Wallet signing ceremonies |
| `bloom:tx/outbox` | `bloom:tx.outbox` | Stage/confirm/inspect EVM transactions |
| `bloom:chain/read` | `bloom:chain` | JSON-RPC chain reads |
| `bloom:vfs/readwrite` | `bloom:vfs.read` / `bloom:vfs.write` | Read/write bloom VFS paths |
| `bloom:env/runtime` | (none) | Runtime environment info |
