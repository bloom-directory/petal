# Live Runtime Testing: Install & Drive a Petal via VFS

After unit tests, integration tests, WASM build, and `petal check` all pass,
the final validation layer is installing the petal into a live bloom runtime
and driving its VFS routes against real upstream APIs. This catches wiring
issues that mock-based tests cannot: capability routing, store/VFS host
binding, gas estimation, real API response shapes, and outbox staging.

## Prerequisites

- bloom debug binary: `target/debug/bloom` (or release)
- petal built: `petal build` succeeds with all WASM components
- bloom home initialized: `~/.bloom/` with `config.toml`, keystore, etc.
- Any required API keys configured under `[petals.runtime.<name>.values]`
  in `~/.bloom/config.toml`
- At least one wallet configured (`bloom wallet list`)

## Step 1: Install the petal

```sh
cd /root/dev/bloom-directory/bloom
./target/debug/bloom petals install /root/dev/bloom-directory/bloom-petal-<name>
```

Install validates every route component's capabilities against `petal.toml`
policy, prints the route tree with declared capabilities, and registers the
package in `~/.bloom/petals/`. The output ends with a listing of all route
paths, their operations, capabilities, and flags.

Verify installation:

```sh
./target/debug/bloom petals ls
# Should show the petal with an app path like petals/<name>/
```

## Step 2: Verify configuration and meta routes

Read-only routes that don't need wallet context can be tested immediately:

```sh
# Settings status (API key configured?)
./target/debug/bloom vfs cat /petals/<name>/settings/status.json

# Route contract metadata
./target/debug/bloom vfs cat /petals/<name>/meta/route-contract.json

# List the petal root
./target/debug/bloom vfs ls /petals/<name>/
```

## Step 3: Create a real intent (upstream API call)

For petals that write intents (DeFi, trading, etc.), create a real session:

```sh
# Plain-text intent body
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new \
  --data 'swap 1.0 usdc to dai'

# JSON intent body (for structured parameters)
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new \
  --data '{"intent":"swap 1.0 usdc to dai","chain":"ethereum"}'
```

**Gotcha:** `bloom vfs write` uses `--data` (not `-d`). The `-d` flag is
rejected.

The petal component executes its `create` handler, which may call the upstream
API, verify the route, run a simulation, and persist the session. On success
the write returns silently (empty stdout). The session ID is stored at
`intents/<wallet>/latest`.

## Step 4: Inspect the created session

```sh
# Get the session ID
INTENT_ID=$(./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/latest)

# Read the human-readable plan
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/plan.md

# Machine-readable status with intent states, policy checks, history
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/status.json

# Route response from upstream API
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/route.json

# Simulation result (eth_call)
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/simulation.json

# Policy check results
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/policy_check.json

# Transaction details (all staged/prepared txs)
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/tx.json

# Ceremony review payload
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/review_intent.json

# List all sessions for this wallet
./target/debug/bloom vfs ls /petals/<name>/intents/<wallet>/
```

## Step 5: Test the lifecycle (confirm, abandon, settlement)

### Confirm (stage transactions into outbox)

```sh
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/$INTENT_ID/confirm \
  --data ''
```

After confirm, check the outbox and status:

```sh
# Status should show state: "staged" with outbox IDs
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/status.json

# Bloom's wallet outbox
./target/debug/bloom vfs ls /wallets/<wallet>/chains/<chain>/outbox/
```

### Abandon (cancel before staging)

```sh
# On a prepared (not yet staged) session:
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/$INTENT_ID/abandon \
  --data ''
```

### Settlement tracking

```sh
./target/debug/bloom vfs cat /petals/<name>/intents/<wallet>/$INTENT_ID/wait_settlement.json
```

Statuses: `not_staged` (no txs staged), `destination_pending` (staged, no
balance change observed), `destination_received` (balance increased),
`unsupported_token` (native token output can't be tracked via erc20_balance).

## Step 6: Test security guards

These should be verified against the live runtime, not just unit tests:

```sh
# Confirm on an abandoned session → should error
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/$ABANDONED_ID/confirm --data ''
# Expected: "cannot confirm a session in terminal state: abandoned"

# Abandon on a staged session → should error
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/$STAGED_ID/abandon --data ''
# Expected: "cannot abandon after an outbox transaction exists"

# Confirm on already-staged session → should succeed silently (idempotent)
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/$STAGED_ID/confirm --data ''

# Wrong wallet → should error
./target/debug/bloom vfs cat /petals/<name>/intents/wrong-wallet/latest
# Expected: "not found"
```

## Step 7: Test input validation

```sh
# Unknown token
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new \
  --data 'swap 100.0 faketoken to eth'
# Expected: "could not resolve token symbol: faketoken"

# Empty body
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new --data ''
# Expected: "empty intent body"

# Unparseable
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new --data 'xyz'
# Expected: "could not parse intent"
```

## Step 8: Test cross-chain (if applicable)

```sh
./target/debug/bloom vfs write /petals/<name>/intents/<wallet>/new \
  --data '{"intent":"swap 5.0 usdc to usdc","chain":"base","destination_chain":"ethereum"}'
```

Verify:
- `cross_chain` policy warning fires in `policy_check.json`
- Token addresses resolve per-chain (USDC on Base ≠ USDC on Ethereum)
- Settlement baseline reads from the destination chain
- Route uses bridge/relay protocol

## Expected behaviors with unfunded wallets

When testing against mainnet with a wallet that has no token balance:

- **Simulation reports failure** — `eth_call` correctly reverts with
  "transfer amount exceeds balance" or "insufficient allowance". This is
  expected, not a bug. The route itself is valid.
- **Gas estimation warns** — bloom logs `estimate_gas failed` with the RPC
  error and falls back to a default gas limit. The transaction still stages.
- **Settlement shows `destination_pending`** — no balance change because the
  transaction can't broadcast without funds.

## Debugging: SDK error message stripping

The petal SDK's `host_err()` function (in `petal-sdk/src/lib.rs`) classifies
host error messages by substring and returns a generic `HostStatus` enum
variant instead of the original message:

```rust
fn host_err(message: String) -> SdkError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not found")     { SdkError::Host(HostStatus::NotFound) }
    else if lower.contains("denied") || lower.contains("permission") { SdkError::Host(HostStatus::Denied) }
    else if lower.contains("invalid")  { SdkError::Host(HostStatus::Invalid) }
    else                               { SdkError::Message(message) }
}
```

This means **any host error whose message contains "invalid" (e.g.
`HostError::Invalid("from must be a 0x-prefixed address")`) is stripped down
to just `"invalid"` with zero diagnostic context.** The petal sees only
`SdkError::Host(HostStatus::Invalid)`, and `SdkError::message()` returns the
string `"invalid"`.

This is the #1 cause of mysterious simulation `"error": "invalid"` results.
The route's `eth_call` may be perfectly valid — the host is rejecting a
parameter formatting issue, but the petal can't tell what went wrong.

### Workaround: diagnostic patch to `chain_read`

Add context to the error before it propagates:

```rust
fn chain_read(&mut self, chain: &str, method: &str, params: &str) -> Result<String, String> {
    petal::sdk::chain_read(chain, method, params).map_err(|error| match error {
        petal::SdkError::Message(msg) => msg,
        other => format!("{:?} (chain={}, method={}, params={})",
            other, chain, method, params),
    })
}
```

This surfaces the params that triggered the rejection, making it possible to
identify which field the host found invalid.

### Verify the route independently

When simulation fails but the route looks correct, verify the `eth_call`
directly via `curl` against the chain's RPC to confirm the transaction data
is valid:

```sh
# Construct eth_call params from the route.json tx fields
curl -s -X POST <rpc_url> -H 'Content-Type: application/json' -d '{
  "jsonrpc":"2.0","method":"eth_call","id":1,
  "params":[{
    "from":"0xWALLET",
    "to":"0xROUTER",
    "data":"0xDATA",
    "value":"0x0"
  }, "latest"]
}'
# If this returns "result":"0x..." — the tx is valid; the petal has a
# param formatting issue in its chain_read call, not a route problem.
```

## Enso API specifics

- API key stored in `~/.bloom/config.toml` under
  `[petals.runtime.enso.values]` as `enso-api-key`.
- Route endpoint: `GET https://api.enso.finance/api/v1/shortcuts/route`
- **Native ETH (0xeee...) routes return 500** — Enso API cannot build
  shortcuts for native ETH input. This is an upstream limitation; use
  WETH (0xC02aaA39...) for testing ETH-like routes.
- Cross-chain routes use the `relay` protocol; same-chain use DEX-specific
  protocols (bitget, barter, uniswap, etc.).
- Route verification checks: tx.to matches expected router, tx.from matches
  wallet address, calldata token parameters match request.
