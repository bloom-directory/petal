# DeFi & Chain Capability Patterns

Concrete patterns for petals that interact with EVM chains — reading token
state, staging transactions, simulating calldata, and verifying settlement.
Observed during the `bloom-petal-enso` build. These supplement
`petal-scaffolding-patterns.md`, which covers the general Host/store/HTTP
shapes.

## Host trait (not struct — testable abstraction)

The reference file shows `BloomHost<'a>` holding `&Ctx`. The enso petal evolved
to a **zero-sized struct implementing a trait**, which allows unit tests
without the SDK:

```rust
// runtime.rs

/// Abstraction over all host capabilities. Production impl calls petal::sdk.
/// Tests can implement a mock Host.
pub trait Host {
    // Store
    fn put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), String>;
    fn get(&mut self, key: &str, max_len: usize) -> Result<Option<Vec<u8>>, String>;
    fn get_secret(&mut self, key: &str, max_len: usize) -> Result<Option<String>, String>;
    fn list(&mut self, prefix: &str, max_len: usize) -> Result<Vec<String>, String>;

    // VFS
    fn vfs_read(&mut self, path: &str, max_len: usize) -> Result<Vec<u8>, String>;

    // HTTP
    fn http_fetch(&mut self, req: &petal::sdk::HttpRequest) -> Result<petal::sdk::HttpResponse, String>;

    // Chain reads
    fn chain_id(&mut self, chain_name: &str) -> Result<u64, String>;
    fn eth_call(&mut self, chain: &str, to: &str, data: &str, from: Option<&str>, value: Option<&str>) -> Result<EthCallResult, String>;
    fn eth_balance_of(&mut self, chain: &str, addr: &str) -> Result<String, String>;
    fn erc20_balance(&mut self, chain: &str, token: &str, addr: &str) -> Result<String, String>;
    fn erc20_decimals(&mut self, chain: &str, token: &str) -> Result<u8, String>;
    fn erc20_allowance(&mut self, chain: &str, token: &str, owner: &str, spender: &str) -> Result<String, String>;

    // Transaction outbox
    fn tx_stage(&mut self, tx: &petal::sdk::EvmTransaction) -> Result<petal::sdk::StagedTransaction, String>;
    fn tx_inspect(&mut self, wallet: &str, chain: &str, id: &str) -> Result<petal::sdk::TxInspection, String>;

    // Settings / env
    fn setting(&mut self, key: &str) -> Result<Option<String>, String>;
    fn now_ms(&mut self) -> u64;
    fn random(&mut self, n: usize) -> Result<Vec<u8>, String>;
}

/// Zero-sized production impl — calls petal::sdk functions directly.
pub struct BloomHost;
```

Key design points:
- `BloomHost` is zero-sized; all SDK calls go through `petal::sdk::*` functions.
- Methods take `&mut self` and return `Result<T, String>` (not `Option<T>`).
- `get_secret` reads from the secret store namespace; `get` reads public.
- Chain methods accept chain name as `&str` (e.g. `"ethereum"`, `"base"`).
- Balance/allowance return decimal string (not hex), ready for comparison.
- `erc20_decimals` returns `u8` directly.

## EvmTransaction struct shape

The SDK's `EvmTransaction` for staging transactions:

```rust
pub struct EvmTransaction {
    pub wallet: String,
    pub chain: String,
    pub to: String,          // hex address 0x...
    pub value_wei: String,   // decimal string, e.g. "0" or "1000000000000000000"
    pub data_hex: String,    // hex calldata 0x...
    pub nonce: Option<String>,
    pub max_fee_per_gas: Option<String>,
    pub max_priority_fee_per_gas: Option<String>,
}
```

`tx_stage` returns a `StagedTransaction`:

```rust
pub struct StagedTransaction {
    pub outbox_id: String,
    pub plan_md: String,
    pub approval: Option<OutboxApproval>,
}

pub struct OutboxApproval {
    pub action_id: String,
    pub ceremony_url: String,
    pub expires_ms: u64,
}
```

`tx_inspect` returns an `OutboxInspection` (not `TxInspection`) with
`state: String`, optional `tx_hash: Option<String>`, and optional
`receipt_json: Option<String>`.

```rust
pub struct OutboxInspection {
    pub outbox_id: String,
    pub state: String,
    pub tx_hash: Option<String>,
    pub receipt_json: Option<String>,
}
```

When `tx_stage` returns an `OutboxApproval`, the session must capture it so
ceremony routes can expose the `action_id` and `ceremony_url` to the user.
Add an `approval` field to `IntentState` and serialize it as JSON.

## ERC-20 allowance flow

Before executing a swap through a router, the router must have sufficient
allowance to pull the input token. The standard flow:

1. **Check allowance** — `erc20_allowance(chain, token, owner, spender)`
2. **Compare** — decimal string comparison (not U256; the values come back as
   decimal strings from the host)
3. **Stage approve** if insufficient — build `approve(spender, max_uint256)`
   calldata using the `0x095ea7b3` selector:

```rust
fn build_approve_calldata(spender_hex: &str) -> String {
    let stripped = spender_hex.strip_prefix("0x").unwrap_or(spender_hex);
    let padded_spender = format!("{:0>64}", stripped.to_ascii_lowercase());
    // approve(address,uint256) selector + padded spender + max uint256
    format!("0x095ea7b3{padded_spender}{}", "f".repeat(64))
}
```

4. **Stage as separate intent** before the route transaction. Both go into the
   session's `intents` vector so they can be inspected and confirmed together.

## Decimal string comparison helpers

Chain reads return decimal strings. For comparisons without pulling in bigint
libraries at the route level:

```rust
/// Returns true if decimal string a < decimal string b.
fn lt_decimal(a: &str, b: &str) -> bool {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    if a.is_empty() && b.is_empty() { return false; }
    if a.len() != b.len() { return a.len() < b.len(); }
    a < b
}
```

## Amount parsing with explicit decimals

Parse human-readable amounts (e.g. "1.5 ETH") into raw smallest-units using
the token's decimals. Handles both integer ("100") and decimal ("1.5") input:

```rust
fn parse_amount(amount: &str, decimals: u8) -> Result<U256, String> {
    // Integer without decimal point → parse directly (already in smallest units)
    // Decimal point → split, pad fractional part to `decimals` length, combine
    // Reject if fractional digits exceed `decimals`
}
```

Use `alloy::primitives::U256` for the arithmetic. The `alloy` crate (with
features `dyn-abi`, `sol-types`, `std`) provides ABI encoding/decoding and
address types.

## Simulation via eth_call

Run a non-committal `eth_call` against the route's target contract to detect
reverts before staging. The `eth_call` Host method returns an `EthCallResult`
with `.success: bool` and `.return_data: String`.

**Extract a shared helper** so both `create()` (pre-session, has `RouteResponse`)
and `simulate_route()` (post-session, has stored `Session`) call the same code:

```rust
/// Simulate directly from a RouteResponse — used by create() before the
/// session is persisted, and by simulate_route() for stored sessions.
pub fn simulate_route_response<H: Host>(
    host: &mut H,
    chain: &str,
    route: &RouteResponse,
) -> serde_json::Value {
    let to = format!("0x{:x}", route.tx.to);
    let data = format!("0x{}", hex::encode(&route.tx.data));
    let from = format!("0x{:x}", route.tx.from);
    let value = route.tx.value.to_string();
    match host.eth_call(chain, &to, &data, Some(&from), Some(&value)) {
        Ok(res) if res.success => json!({"success": true, "return_data": res.return_data}),
        Ok(res) => {
            let msg = decode_error_string(&res.return_data)
                .unwrap_or_else(|| res.return_data.clone());
            json!({"success": false, "decoded_error": {"message": msg}})
        }
        Err(e) => json!({"success": false, "error": e}),
    }
}

/// Simulate for a stored session.
pub fn simulate_route<H: Host>(host: &mut H, sess: &Session) -> serde_json::Value {
    match sess.route.as_ref() {
        Some(route) => simulate_route_response(host, &sess.chain, route),
        None => json!({"success": false, "error": "session has no route to simulate"}),
    }
}
```

The standard `Error(string)` ABI selector is `0x08c379a0`. Decode the string
from the revert data (skip 4-byte selector, read offset + length + data):

```rust
pub fn decode_error_string(hex_data: &str) -> Option<String> {
    // Must start with Error(string) selector 0x08c379a0
    // Layout: selector (4 bytes) + offset (32) + length (32) + data
}
```

## Settlement verification via balance deltas

After broadcasting, verify the destination token balance increased:

1. **Record baseline** — during `create`, call `erc20_balance` and store as
   `observed_before` in the session.
2. **Check after broadcast** — call `erc20_balance` again and compute
   `delta = current - before`.
3. **Status logic**: `delta > 0` → `destination_received`; else if staged but
   `delta == 0` → `destination_pending`.
4. **Native token caveat** — `erc20_balance` can't read native ETH balances.
   Use `eth_balance_of` instead, but note that gas costs reduce the balance,
   so the delta may be negative for native-to-native swaps.

Decimal string subtraction (for computing delta):

```rust
fn sub_decimal(a: &str, b: &str) -> String {
    // Grade-school subtraction on decimal digit vectors.
    // Returns "0" if b > a (no negative deltas).
}
```

## Multi-intent session model

A session can carry multiple staged transactions (approve + route, or multiple
routes for cross-chain). Each intent has its own outbox tracking:

```rust
pub struct PreparedIntent {
    pub label: String,         // "approve" or "route"
    pub to: String,            // hex address
    pub value_wei: String,     // decimal
    pub data_hex: String,      // hex calldata
    pub chain: String,
    pub approve_token: Option<String>,
    pub approve_spender: Option<String>,
}

pub struct IntentState {
    pub index: usize,
    pub status: String,        // "prepared" | "staged" | "submitted" | "confirmed"
    pub outbox_id: Option<String>,
    pub tx_hash: Option<String>,
    pub depends_on: Option<String>,  // outbox_id of prerequisite intent (approve → route)
    pub approval: Option<serde_json::Value>, // ceremony approval from tx_stage
    pub updated_ms: u64,
}
```

The `confirm` handler stages all intents sequentially, persisting after each
stage for crash recovery. Re-verify route input integrity before staging.

**Critical: `confirm()` must check `sess.terminal()` before any work.** A
missing guard allows abandoned or settled sessions to be revived — staging
transactions on a session the user explicitly cancelled. This is a common
omission because the idempotency check (already staged → return Ok) feels
sufficient, but it only covers the "staged" state, not terminal states:

```rust
pub fn confirm<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String> {
    let mut sess = load(host, wallet, id)?;
    // Terminal guard — MUST come before idempotency check
    if sess.terminal() {
        return Err(format!("session is terminal: {}", sess.state));
    }
    // Idempotency — already fully staged
    if sess.intent_states.iter().all(|s| s.status == "staged") {
        return Ok(());
    }
    // ... stage intents
}
```

## Address comparison: checksum vs lowercase hex

EVM addresses can appear in two common formats:
- **Lowercase hex**: `0x0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a` (from `format!("0x{:x}", addr)`)
- **Checksummed (EIP-55)**: `0x0A0a0A0a0A0a0A0a0A0a0A0a0A0a0A0a0A0a0A0a` (mixed case, from VFS/wallet sources)

Comparing these with `==` or `!=` will silently fail when the case differs.
**Always use `eq_ignore_ascii_case` for address comparison:**

```rust
// WRONG — fails when one address is checksummed and the other lowercase:
if from_hex != *wallet_addr {
    return Err("wallet mismatch".into());
}

// CORRECT:
if !from_hex.eq_ignore_ascii_case(wallet_addr) {
    return Err("wallet mismatch".into());
}
```

**Also apply `eq_ignore_ascii_case` to chain names**, not just addresses.
Chain names from different sources can have different casing (`"Ethereum"`
from a VFS route parameter vs `"ethereum"` from the session record). Using
`!=` for chain comparison falsely flags same-chain swaps as cross-chain:

```rust
// WRONG — case-sensitive comparison:
if destination_chain != sess.chain {
    // falsely triggers cross-chain policy warning
}

// CORRECT:
if !destination_chain.eq_ignore_ascii_case(&sess.chain) {
    // genuinely cross-chain
}
```

This bug was caught by adversarial integration tests that fed mixed-case
chain names through the workflow. Check every comparison involving addresses
or chain names — grep for `!=` and `==` on these types.

## Token address tables must be chain-specific

ERC-20 tokens are deployed at **different addresses on each chain**. A static
token symbol → address lookup table is correct for one chain and wrong for
every other. This is a silent bug: the route request is built with an address
that doesn't exist on the target chain, the upstream API (Enso, 0x, etc.)
rejects it with an opaque error, and the petal has no way to know the address
was wrong rather than the request.

**Rule:** If the petal supports multiple chains, the token table must be keyed
by `(chain, symbol)`, not just `symbol`:

```rust
// WRONG — same address for all chains:
fn resolve_token(symbol: &str) -> &str {
    match symbol {
        "dai" => "0x6b1755...bb3d",  // Ethereum DAI — doesn't exist on Base!
        ...
    }
}

// CORRECT — per-chain lookup:
fn resolve_token(chain: &str, symbol: &str) -> Result<&str, String> {
    match (chain, symbol) {
        ("ethereum", "dai")  => Ok("0x6b1755...bb3d"),
        ("base", "dai")      => Ok("0x..."),  // or return error if unsupported
        ("base", "usdc")     => Ok("0x8335..."),
        ("ethereum", "usdc") => Ok("0xa0b8..."),
        ...
        _ => Err(format!("could not resolve token symbol: {symbol} on {chain}")),
    }
}
```

**Common gotchas:**
- USDC on Ethereum (`0xa0b8...eb48`) ≠ USDC on Base (`0x8335...cd6e`)
- DAI on Ethereum (`0x6b1755...bb3d`) does not exist on Base
- WETH on Ethereum (`0xc02a...6cc2`) ≠ WETH on Base (`0x4200...0006`)
- When a token genuinely doesn't exist on a chain, return an error rather
  than falling back to the Ethereum address

## Route integrity verification

Before staging, verify the route transaction's calldata matches the original
request (anti-tampering). The `RouteResponse` should include an
`input_matches_request(&RouteRequest) -> bool` method that checks:

- `tx.to` matches the expected router address
- `tx.from` matches the wallet address
- Decoded calldata parameters (token in, token out, amount) match the request

This runs at both `create` time (before saving the session) and `confirm` time
(before staging), so a tampered stored session cannot be executed.

## Ceremony lifecycle routes

DeFi petals that stage transactions through bloom's outbox should expose the
full ceremony lifecycle, not just the original create/confirm/wait/settle
paths. Comparing against sibling petals (near, polymarket) reveals five
additional endpoints that are now standard:

| Route | Kind | Purpose |
| --- | --- | --- |
| `review_intent.json` | read | Ceremony review payload — tx details, policy status, approval data for the ceremony system to present |
| `outbox.json` | read | Live outbox state for all staged intents (calls `tx_inspect` per outbox ID) |
| `receipt.json` | read | Terminal receipt — all tx hashes, settlement status, full history |
| `abandon` | writable | Cancel a session before any outbox transaction exists (refuses if any intent is staged) |
| `latest` | read (wallet-level) | Convenience shortcut — resolves to the most recently created session ID |

### Capturing ceremony approval in confirm

The `confirm` workflow must capture the `OutboxApproval` from each
`tx_stage` return and store it in the session:

```rust
let staged = host.tx_stage(&evm_tx)?;
if let Some(state) = sess.intent_states.get_mut(idx) {
    state.status = "staged".into();
    state.outbox_id = Some(staged.outbox_id.clone());
    state.approval = staged.approval.as_ref().map(|a| serde_json::json!({
        "action_id": a.action_id,
        "ceremony_url": a.ceremony_url,
        "expires_ms": a.expires_ms,
    }));
    state.updated_ms = now;
}
```

### Session helper methods

```rust
impl Session {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(),
            "settled_success" | "settled_failed" | "abandoned")
    }

    /// Last staged intent's outbox ID (the primary route tx).
    pub fn primary_outbox_id(&self) -> Option<&str> {
        self.intent_states.iter().rev()
            .find_map(|s| s.outbox_id.as_deref())
    }

    /// Last staged intent's tx hash.
    pub fn primary_tx_hash(&self) -> Option<&str> {
        self.intent_states.iter().rev()
            .find_map(|s| s.tx_hash.as_deref())
    }
}
```

### Abandon workflow

```rust
pub fn abandon<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String> {
    let now = host.now_ms();
    let mut sess = load(host, wallet, id)?;
    if sess.intent_states.iter().any(|s| s.outbox_id.is_some()) {
        return Err("cannot abandon after an outbox transaction exists".into());
    }
    if sess.terminal() { return Err("session is already terminal".into()); }
    sess.transition(now, "abandoned", "user abandoned before staging");
    save(host, &sess)
}
```

### Latest pointer

Store a `latest` key at session creation time so `intents/{wallet}/latest`
can resolve without listing:

```rust
let _ = host.put(&format!("intents/{wallet}/latest"), id.as_bytes(), false);
```
