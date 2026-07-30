# Petal Scaffolding Implementation Patterns

Concrete code patterns that recur when creating or migrating a Bloom petal.
The SKILL.md describes the high-level workflow; this reference shows the
actual implementation shapes observed across `bloom-petal-polymarket`,
`bloom-petal-near`, and `bloom-petal-enso`.

## Sync HTTP adapter (replacing async reqwest)

Bloom native crates use async `reqwest`. Petals must use the synchronous
`petal::sdk::http_fetch`. The migration is mechanical but touches every API
call:

```rust
use petal::sdk::{http_fetch, HttpRequest, HttpResponse};

fn enso_route(base_url: &str, api_key: &str, body: &RouteRequest) -> Result<RouteResponse, ApiError> {
    let req = HttpRequest {
        method: "POST".into(),
        url: format!("{}/api/v1/route", base_url),
        headers: vec![
            ("Authorization".into(), format!("Bearer {}", api_key)),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: Some(serde_json::to_vec(body).map_err(|e| ApiError::Serialize(e.to_string()))?),
    };
    let resp = http_fetch(&req).map_err(|e| ApiError::Http(e))?;
    if resp.status != 200 {
        return Err(ApiError::Status(resp.status, String::from_utf8_lossy(&resp.body).into_owned()));
    }
    serde_json::from_slice(&resp.body).map_err(|e| ApiError::Deserialize(e.to_string()))
}
```

Key points:
- `HttpRequest` and `HttpResponse` are plain structs with `String` / `Vec<u8>` fields.
- `headers` is `Vec<(String, String)>`, not a map.
- `http_fetch` returns synchronously — no `.await`, no async fn, no tokio.
- Error responses must be caught by status code before deserializing the body
  as the success type.
- Shared HTTP helpers should resolve the base URL from a fixed enum or const,
  never from caller-supplied strings, to satisfy the network-egress safety rule.

## BloomHost runtime (Host trait implementation)

The shared route crate (`route/src/runtime.rs`) implements a `Host` struct
that the petal SDK calls into. It wraps `&Ctx` and provides typed methods for
each capability surface:

```rust
pub struct BloomHost<'a> {
    ctx: &'a petal::Ctx,
}

impl<'a> BloomHost<'a> {
    pub fn new(ctx: &'a petal::Ctx) -> Self {
        Self { ctx }
    }

    // Store helpers
    pub fn store_get(&self, key: &str) -> Option<Vec<u8>> {
        petal::sdk::store_get(self.ctx, key)
    }
    pub fn store_set(&self, key: &str, value: &[u8]) {
        petal::sdk::store_set(self.ctx, key, value);
    }

    // VFS read
    pub fn vfs_read(&self, path: &str) -> Option<Vec<u8>> {
        petal::sdk::vfs_read(self.ctx, path)
    }

    // Transaction outbox
    pub fn tx_stage(&self, chain_id: u64, to: &str, calldata: &str, value: &str) -> Result<String, String> {
        petal::sdk::tx_stage(self.ctx, chain_id, to, calldata, value)
            .map_err(|e| e.to_string())
    }
    pub fn tx_inspect(&self, id: &str) -> Option<TxStatus> {
        // read back staged/submitted status
    }
}
```

The Host is constructed fresh in each route file handler from `&Ctx`. It does
not hold mutable state — durable state lives in the store.

> **Note:** the enso petal evolved this to a **trait-based** `Host` abstraction
> (zero-sized `BloomHost` implementing a `Host` trait) that enables unit
> testing without the SDK, plus chain capability methods (`erc20_balance`,
> `erc20_decimals`, `erc20_allowance`, `eth_call`). See
> `references/defi-chain-patterns.md` for the updated trait shape and chain
> helpers.

## Credential resolution (settings module)

Petals that need an API key follow this resolution order:

1. **Private store** — `store_get("credentials/<service>-api-key")` (user-set
   via `settings/api-key` writable route).
2. **Runtime setting** — `store_get("settings/<service>/api-key")` (set by
   runtime config or env var `BLOOM_<SERVICE>_KEY`).
3. **Environment** — `petal::sdk::env_var("BLOOM_<SERVICE>_KEY")` as fallback.

Pattern in `route/src/settings.rs`:

```rust
pub enum CredentialSource {
    PrivateStore,   // user wrote via settings/ route
    RuntimeSetting, // from config
    Environment,    // BLOOM_*_KEY
    Missing,
}

pub fn resolve_api_key(host: &BloomHost) -> (Option<String>, CredentialSource) {
    if let Some(key) = host.store_get("credentials/enso-api-key") {
        return (Some(String::from_utf8_lossy(&key).into_owned()), CredentialSource::PrivateStore);
    }
    if let Some(key) = host.store_get("settings/enso/api-key") {
        return (Some(String::from_utf8_lossy(&key).into_owned()), CredentialSource::RuntimeSetting);
    }
    if let Some(key) = petal::sdk::env_var("BLOOM_ENSO_KEY") {
        return (Some(key), CredentialSource::Environment);
    }
    (None, CredentialSource::Missing)
}
```

The `settings/api-key.rs` route file is a writable file whose `write` handler
stores the key into the private store namespace. The `settings/status.json.rs`
route reports which source is active (without exposing the key value).

## Durable session lifecycle

Multi-step operations (trades, swaps, orders) follow a create → store →
confirm → settle lifecycle stored as durable state in the petal store:

```
POST intents/<wallet>/new
  → parse natural language intent
  → call upstream API for routing/quote
  → create IntentSession with status="routed"
  → store at "intents/<wallet>/<id>"
  → return route preview

GET  intents/<wallet>/<id>/*
  → read back route.json, plan.md, tx.json, simulation.json, status.json

POST intents/<wallet>/<id>/confirm
  → load session, verify status="routed"
  → stage calldata via tx_outbox
  → update status="staged"
  → return settlement reference
```

Session state is a plain struct serialized as JSON in the store:

```rust
#[derive(Serialize, Deserialize)]
pub struct IntentSession {
    pub id: String,
    pub wallet: String,
    pub chain_id: u64,
    pub status: SessionStatus,
    pub intent_text: String,
    pub route_json: serde_json::Value,
    pub tx_calldata: Option<String>,
    pub tx_to: Option<String>,
    pub tx_value: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub enum SessionStatus {
    Routed,    // route preview ready, awaiting confirmation
    Staged,    // tx staged to outbox, awaiting broadcast
    Submitted, // tx broadcast to chain
    Confirmed, // tx confirmed on chain
    Failed,    // error occurred
}
```

Each route file (`intent.txt.rs`, `route.json.rs`, `tx.json.rs`,
`status.json.rs`, etc.) reads the session from store and projects one field.
The confirm route is the only one that mutates session state.

## Route file listing (store-backed directories)

Dynamic directories listing store-backed entries use `ctx_list`:

```rust
petal::route_file!(
    spec: petal::store_dir_spec(),
    list: ctx_list: |ctx: &petal::Ctx| {
        let host = BloomHost::new(ctx);
        let wallet = match petal::param(ctx, "wallet") {
            Ok(w) => w,
            Err(resp) => return resp,
        };
        // scan store keys for this wallet's intents
        let entries = host.store_list(&format!("intents/{}/", wallet));
        entries.into_iter()
            .map(|id| petal::dir(id))
            .collect()
    },
);
```

Note: `store_list` may not be available on all SDK revisions. Check the
pinned SDK's public API. If unavailable, enumerate from a stored index key
(e.g. `"intents/<wallet>/_index"` containing a JSON array of IDs).

## Route architecture check script

Every petal repo should include `scripts/check-route-architecture.sh` that
greps the shared crate for forbidden path-dispatch patterns:

```sh
#!/bin/bash
set -euo pipefail
ROUTE_SRC="route/src"

FORBIDDEN=(
    "current_route_path"
    "current_route_canonical_path"
)

CODE=0
for pattern in "${FORBIDDEN[@]}"; do
    if grep -rn "$pattern" "$ROUTE_SRC" --include="*.rs"; then
        echo "ERROR: forbidden path-dispatch accessor '$pattern' found in shared crate"
        CODE=1
    fi
done

exit $CODE
```

## PETAL_REV pinning

The petal SDK git revision is pinned in three places. All three must match:

1. `route/Cargo.toml`:
   ```toml
   [dependencies]
   petal = { git = "https://github.com/bloom-directory/petal", rev = "4f6fb57063a70f95cba288f68bdc139e3ecac7a5" }
   ```
2. `scripts/build.sh`:
   ```sh
   export PETAL_REV="4f6fb57063a70f95cba288f68bdc139e3ecac7a5"
   ```
3. `petal-build.toml`:
   ```toml
   [sdk]
   package = "bloom-petal-sdk"
   git = "https://github.com/bloom-directory/petal"
   rev = "4f6fb57063a70f95cba288f68bdc139e3ecac7a5"
   ```

Find the current revision from the petal repo:
```sh
cd petal && git log --oneline -1
```

## Static token / address tables

When migrating from bloom monorepo, native crates depend on `bloom_proto` for
token symbols, chain IDs, and address constants. Petals cannot depend on
bloom_proto. Replace with a static table in the shared crate:

```rust
pub fn resolve_token(chain_id: u64, symbol: &str) -> Option<&'static str> {
    match (chain_id, symbol.to_uppercase().as_str()) {
        (1, "WETH") => Some("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        (1, "USDC") => Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        (8453, "WETH") => Some("0x4200000000000000000000000000000000000006"),
        (8453, "USDC") => Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        // ... expand as needed
        _ => None,
    }
}
```

This is a known simplification — the monorepo's `bloom_proto::tokens` has
hundreds of entries. Start small and expand based on usage.

## Manifest file shapes

### petal.toml

```toml
name = "enso"
display = "DeFi Routing"
description = "Route and execute token swaps via Enso"

[caps]
allowed = [
    "bloom:http",
    "bloom:store",
    "bloom:tx.outbox",
    "bloom:chain",
    "bloom:vfs.read",
]

[[net.allow]]
host = "api.enso.finance"
methods = ["GET", "POST"]
paths = ["/api/"]

[[net.allow]]
host = "quoter.api.enso.build"
methods = ["POST"]
paths = ["/"]
```

### petal-build.toml

```toml
[sdk]
package = "bloom-petal-sdk"
git = "https://github.com/bloom-directory/petal"
rev = "<sha>"

[build]
route_crate = "route"
routes_dir = "route/files"
```

### route/Cargo.toml

```toml
[package]
name = "bloom-petal-enso-route"
version = "0.1.0"
edition = "2021"

[lib]
name = "route"
path = "src/lib.rs"

[dependencies]
petal = { git = "https://github.com/bloom-directory/petal", rev = "<sha>" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```
