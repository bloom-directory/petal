# Integration Testing for Petal Workflows

How to test petal workflow logic end-to-end without the SDK or real APIs.
The key insight: if the `Host` trait is the abstraction boundary (see
`defi-chain-patterns.md` § Host trait), then a mock `Host` implementation
exercises the full workflow — create, confirm, settle, abandon — in pure
Rust with no WASM, no SDK, no network.

This pattern found 3 real production bugs across two rounds of adversarial
testing: a case-sensitive address comparison (round 1), a missing
terminal-state guard allowing abandoned session revival (round 2), and a
case-sensitive chain name comparison falsely triggering cross-chain policy
(round 2). All compiled clean and passed unit tests.

## Prerequisites

The shared route crate must define a `Host` trait (not a concrete struct
with SDK calls baked in). See `defi-chain-patterns.md` for the trait shape.
The workflow functions take `H: Host` as a generic parameter:

```rust
pub fn create<H: Host>(host: &mut H, wallet: &str, intent: &str) -> Result<Session, String>;
pub fn confirm<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String>;
pub fn abandon<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String>;
```

## Mock Host implementation

Implement `Host` with in-memory state. The mock owns a `HashMap` for the
store, canned HTTP responses keyed by URL substring, and canned chain call
results:

```rust
use std::collections::HashMap;
use std::cell::RefCell;

struct MockHost {
    store: HashMap<String, Vec<u8>>,
    secrets: HashMap<String, String>,
    http_responses: HashMap<String, (u16, Vec<u8>)>,  // url_substring → (status, body)
    staged_txs: Vec<petal::sdk::EvmTransaction>,
    time: u64,
}

impl Host for MockHost {
    fn put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), String> {
        if secret {
            self.secrets.insert(key.into(), String::from_utf8_lossy(value).into_owned());
        } else {
            self.store.insert(key.into(), value.to_vec());
        }
        Ok(())
    }

    fn get(&mut self, key: &str, _max_len: usize) -> Result<Option<Vec<u8>>, String> {
        Ok(self.store.get(key).cloned())
    }

    fn http_fetch(&mut self, req: &petal::sdk::HttpRequest) -> Result<petal::sdk::HttpResponse, String> {
        // Match by URL substring — first match wins
        for (pattern, (status, body)) in &self.http_responses {
            if req.url.contains(pattern) {
                return Ok(petal::sdk::HttpResponse {
                    status: *status,
                    headers: vec![],
                    body: body.clone(),
                });
            }
        }
        Err(format!("no mock for {}", req.url))
    }

    fn tx_stage(&mut self, tx: &petal::sdk::EvmTransaction) -> Result<petal::sdk::StagedTransaction, String> {
        let id = format!("stage-{}", self.staged_txs.len());
        self.staged_txs.push(tx.clone());
        Ok(petal::sdk::StagedTransaction {
            outbox_id: id,
            plan_md: "mock plan".into(),
            approval: None,
        })
    }

    fn now_ms(&mut self) -> u64 { self.time }
    // ... implement remaining trait methods with defaults or canned values
}
```

## Test patterns

### Full lifecycle: create → confirm → verify staged

```rust
#[test]
fn full_lifecycle_erc20() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    host.mock_allowance("1000000000000000000000000"); // sufficient

    // Create
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();
    assert_eq!(sess.state, "routed");
    assert!(sess.route.is_some());

    // Confirm
    confirm(&mut host, WALLET, &sess.id).unwrap();

    // Verify staged
    let stored = load(&mut host, WALLET, &sess.id).unwrap();
    assert!(stored.intent_states.iter().all(|s| s.status == "staged"));
    assert!(stored.intent_states.iter().all(|s| s.outbox_id.is_some()));
}
```

### ERC-20 approve flow (zero allowance → two intents)

```rust
#[test]
fn erc20_zero_allowance_creates_approve() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    host.mock_allowance("0"); // forces approve intent

    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();
    assert_eq!(sess.intent_states.len(), 2); // approve + route
    assert_eq!(sess.intent_states[0].label, "approve");
    assert_eq!(sess.intent_states[1].label, "route");
}
```

### Idempotency

```rust
#[test]
fn confirm_is_idempotent() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();

    confirm(&mut host, WALLET, &sess.id).unwrap();
    let staged = host.staged_txs.len();

    // Second confirm should be a no-op
    confirm(&mut host, WALLET, &sess.id).unwrap();
    assert_eq!(host.staged_txs.len(), staged); // no new txs
}
```

### Wrong wallet can't access session

```rust
#[test]
fn wrong_wallet_cannot_load_session() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();

    let result = load(&mut host, "0xdifferent", &sess.id);
    assert!(result.is_err());
}
```

### Abandon before vs after staging

```rust
#[test]
fn abandon_before_staging_succeeds() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();

    abandon(&mut host, WALLET, &sess.id).unwrap();
    let stored = load(&mut host, WALLET, &sess.id).unwrap();
    assert_eq!(stored.state, "abandoned");
}

#[test]
fn abandon_after_staging_rejected() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();
    confirm(&mut host, WALLET, &sess.id).unwrap();

    let result = abandon(&mut host, WALLET, &sess.id);
    assert!(result.is_err());
}
```

## Test data: mock API responses

Pre-build JSON fixtures matching the upstream API's response shape. Store
them as inline `serde_json::json!` blocks or load from `tests/fixtures/`:

```rust
fn valid_route_json() -> serde_json::Value {
    json!({
        "route": [{
            "protocol": "uniswap-v3",
            "tx": {
                "to": "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45",
                "from": format!("0x{}", "0a".repeat(20)),
                "data": "0x5ae401dc...",
                "value": "0",
                "gas": "150000"
            }
        }]
    })
}
```

## Bug hunting: systematic adversarial test writing

After basic lifecycle tests pass, write a second round of deliberately
adversarial tests to find real bugs. This approach found 3 production bugs
across two rounds in the enso petal — all compiled clean and passed unit tests.

### Methodology

1. **Enumerate every state machine guard.** For each workflow function
   (`create`, `confirm`, `abandon`, `wait_settlement`), list every early-return
   and error path. Ask: what state does each guard check? What states are
   *not* guarded?
2. **Write tests that hit unguarded paths.** If `confirm()` checks for
   "staged" and "confirmed" but has no terminal-state check, write a test that
   creates → abandons → confirms. If the abandoned session comes back to life,
   that's a bug.
3. **Test case sensitivity everywhere.** Write tests where the wallet address
   is checksummed (mixed case) in one place and lowercase hex in another.
   Write tests where chain names have different casing (`"Ethereum"` vs
   `"ethereum"`). If `!=` is used instead of `eq_ignore_ascii_case`, the test
   will catch it.
4. **Test multi-session isolation.** Create two sessions for the same wallet,
   confirm one, and verify the other is unaffected. This requires unique
   session IDs — see the `rand_counter` MockHost enhancement below.
5. **Test partial failure recovery.** Configure the mock to fail after N
   staging calls (`stage_fail_after`). Verify the session is left in a
   recoverable state and retrying `confirm()` completes the remaining intents.
6. **Test input validation exhaustively.** Unknown tokens, invalid intent
   strings, mismatched chain IDs between create and confirm, tampered route
   responses (wrong token_in address, wrong amount, wrong value).

### Bugs this methodology found

| Round | Bug | Severity | Pattern |
| --- | --- | --- | --- |
| 1 | `confirm()` address comparison used `!=` instead of `eq_ignore_ascii_case` | critical | case sensitivity |
| 2 | `confirm()` had no terminal-state guard — abandoned sessions could be revived | critical | missing guard |
| 2 | Cross-chain comparison in `create()` and `confirm()` used case-sensitive `!=` for chain names | policy | case sensitivity |

### Bug pattern: missing terminal-state guards

Workflow functions often check for idempotent states ("already staged" →
return Ok) but forget to check terminal states. A session that was abandoned
or settled should reject all further operations:

```rust
// WRONG — only checks idempotency, not terminal:
pub fn confirm<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String> {
    let mut sess = load(host, wallet, id)?;
    if sess.intent_states.iter().all(|s| s.status == "staged") {
        return Ok(()); // idempotent — but doesn't block abandoned sessions!
    }
    // ... proceeds to stage transactions on an abandoned session
}

// CORRECT:
pub fn confirm<H: Host>(host: &mut H, wallet: &str, id: &str) -> Result<(), String> {
    let mut sess = load(host, wallet, id)?;
    if sess.terminal() {
        return Err(format!("session is terminal: {}", sess.state));
    }
    if sess.intent_states.iter().all(|s| s.status == "staged") {
        return Ok(()); // idempotent
    }
    // ...
}
```

Test to catch this:

```rust
#[test]
fn confirm_on_abandoned_session_rejected() {
    let mut host = MockHost::new();
    host.mock_route_response(valid_route_json());
    let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();
    abandon(&mut host, WALLET, &sess.id).unwrap();

    let result = confirm(&mut host, WALLET, &sess.id);
    assert!(result.is_err(), "confirm on abandoned session must fail");
}
```

## Enhanced MockHost for adversarial testing

The basic MockHost (shown above) handles store, HTTP, and tx staging. For
bug hunting, extend it with controllable failure modes:

```rust
struct MockHost {
    // ... basic fields from above ...

    // Bug-hunting extensions:
    rand_counter: Cell<u64>,           // unique session IDs for multi-session tests
    balance_override: Option<String>,  // force a specific balance (settlement testing)
    stage_fail_after: Option<usize>,   // fail tx_stage after N successful stages
    stage_count: Cell<usize>,          // tracks how many stages have occurred
    http_status_override: Option<u16>, // force HTTP error status
}
```

### rand_counter: unique session IDs

The basic mock's `random()` returns identical bytes every call — two sessions
for the same wallet get the same ID, causing silent collisions. Fix with a
counter:

```rust
fn random(&mut self, n: usize) -> Result<Vec<u8>, String> {
    let count = self.rand_counter.get();
    self.rand_counter.set(count + 1);
    Ok(count.to_le_bytes().to_vec())
}
```

### stage_fail_after: partial failure testing

To test that a session with multiple intents (approve + route) recovers when
the second staging fails:

```rust
fn tx_stage(&mut self, tx: &EvmTransaction) -> Result<StagedTransaction, String> {
    let count = self.stage_count.get();
    self.stage_count.set(count + 1);
    if let Some(fail_after) = self.stage_fail_after {
        if count >= fail_after {
            return Err("simulated staging failure".into());
        }
    }
    // ... normal staging
}
```

### balance_override: settlement verification testing

Set a specific balance to simulate successful settlement (balance increase):

```rust
fn erc20_balance(&mut self, chain: &str, token: &str, addr: &str) -> Result<String, String> {
    if let Some(ref bal) = self.balance_override {
        return Ok(bal.clone());
    }
    Ok("0".into())
}
```

## Test data gotcha: decimal format for amounts

When parsing human-readable intent strings like `"swap 100.0 usdc"`, the
amount parser treats raw integers as already-scaled smallest units. Use
decimal notation (`"100.0"`) so the parser scales by token decimals:

```rust
// WRONG — "100" is treated as 100 wei, not 100 USDC:
let sess = create(&mut host, WALLET, "swap 100 usdc").unwrap();

// CORRECT — "100.0" is parsed as 100 * 10^6 (USDC has 6 decimals):
let sess = create(&mut host, WALLET, "swap 100.0 usdc").unwrap();
```

This is a test-only gotcha — production intents always come through natural
language parsing that handles both formats. But integration tests with
hardcoded intent strings must use the decimal form.

## What integration tests catch that unit tests don't

- **Address format mismatches** (checksum vs lowercase hex) — caught a real
  bug where `confirm()` compared addresses with `!=` instead of
  `eq_ignore_ascii_case`. Unit tests used the same address format everywhere;
  integration tests exposed the mismatch when the VFS layer produces
  checksummed addresses.
- **Missing terminal-state guards** — `confirm()` checked for idempotent
  states but not terminal states, allowing abandoned sessions to be revived.
  Only an adversarial test (create → abandon → confirm) caught this.
- **Case-sensitive chain comparison** — chain names from different sources
  (`"Ethereum"` vs `"ethereum"`) falsely triggered cross-chain policy when
  compared with `!=`.
- **Session serialization roundtrip** — fields that survive in memory but
  lose precision or type when serialized through `serde_json` and back.
- **State machine transitions** — skipping a state, or allowing an invalid
  transition (e.g., confirming an already-staged session).
- **Store key scoping** — wallet isolation bugs where session keys collide
  or leak across wallets.
- **Multi-intent ordering** — approve must be staged before route;
  dependency tracking via `depends_on` field.
- **Route integrity violations** — tampered route responses (wrong token,
  wrong amount) that should be rejected at confirm time but pass if
  `input_matches_request` is missing or incomplete.

## Limitations

Integration tests exercise the workflow layer only. They do NOT test:
- WASM component compilation (route files compiled as separate crates)
- SDK bindings (`petal::sdk::*` function behavior)
- Real upstream API responses (schema changes, rate limits, errors)
- Chain state (real balances, allowances, confirmations)
- Bloom VFS dispatch (petal mounting, route resolution)

For those, see the layered validation in SKILL.md § Validate in layers.
