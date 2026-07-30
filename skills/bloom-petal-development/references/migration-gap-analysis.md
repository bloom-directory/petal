# Migration Gap Analysis Methodology

After porting a native bloom crate to a petal scaffold, a systematic gap
analysis catches the 30+ behavioral gaps that compile and test fine but
represent missing functionality. Observed during the `bloom-petal-enso`
migration where the initial scaffold (38 files, 39 tests passing) still had
38 critical gaps found by this method.

## When to run

After the initial port compiles, passes host-side unit tests, and the route
files produce reasonable output — but **before** declaring the migration
complete or removing the old code from bloom.

## Method

### 1. Map every VFS path from the original handler

Read the original VFS handler (`crates/bloom-vfs/src/handlers/<name>.rs`)
line by line. List every `match` arm in `lookup`, `list`, `read`, and
`write`. Each path becomes a checklist entry:

```
✓ defi/intents/<wallet>/new          → route/files/intents/[wallet]/new.rs
✓ defi/intents/<wallet>/<id>/tx.json → route/files/intents/[wallet]/[id]/tx.json.rs
✗ defi/intents/<wallet>/<id>/policy  → MISSING
```

### 2. Compare create flow logic

Read the original session creation function (e.g. `create_session` in
`defi.rs`). For each decision point, check if the petal's `workflow.rs::create`
replicates it:

- Input validation (amount > 0, valid token symbols, supported chains)
- Natural language intent parsing (all input shapes the original accepts)
- Token symbol → address resolution (coverage of the token table)
- Upstream API call construction (same endpoint, same body fields)
- Response validation (input_matches_request or equivalent)
- Allowance check and approve staging (for ERC-20 flows)
- Simulation before staging
- Policy evaluation (valuation thresholds, receiver classification)
- Session persistence (all fields stored, correct key pattern)
- Plan markdown generation (completeness of human-readable output)

### 3. Compare confirm flow logic

Read the original confirm function. For each safety check:

- Wallet ownership verification (does the caller own this wallet?)
- Status check (can only confirm from the right state?)
- Route integrity re-verification (calldata hasn't been tampered)
- Chain ID verification (source chain matches)
- Multi-intent sequential staging (dependencies respected)
- Per-intent state tracking and persistence

### 4. Compare read route outputs

For each read route file, compare the output shape against what the original
handler returned:

- Does it return the same fields?
- Does it handle error cases (not found, missing data) the same way?
- Does it re-compute live data (e.g. re-run simulation, check settlement)
  or return stale stored data?

### 5. Categorize gaps

```
## Critical (blocks cutover)
- Sequential dependency: approve tx must mine before route tx
- Settlement verification: no destination chain balance delta check
- tx.json returns single tx, not full intent list with approve

## Important (should fix before cutover)
- Simulation not wired into create flow
- Token registry only has ~20 entries (original has hundreds)
- Plan markdown missing policy section

## Nice-to-have (can defer)
- Address book integration for receiver alias
- Full revert decoder beyond Error(string)
- Bundle endpoint support
```

### 6. Track closure

Update the gap analysis file as items close. Use it as the source of truth
for the completion plan (`PLAN.md`).

## Common gap patterns

These recur across migrations:

1. **Write-time safety checks missing at confirm** — The original handler
   re-validates everything at confirm time. Ports often skip this because
   the session is already stored, but a tampered session must not execute.

2. **Sequential transaction dependencies** — Multi-tx flows (approve then
   route) need explicit dependency tracking. The second tx should not be
   staged until the first is confirmed.

3. **Live re-computation on read** — Routes like `simulation.json` should
   re-run the eth_call on each read, not return a stored snapshot. Routes
   like `settlement.json` should check current balances, not stored deltas.

4. **Token table coverage** — Static tables in the petal start small. The
   original `bloom_proto::tokens` registry has hundreds of entries. Identify
   which chains/tokens are actually used and prioritize those.

5. **Human-readable output completeness** — `plan.md` should include all
   information a user needs to make a decision: receiver, router, protocols,
   policy checks, token amounts, slippage, tx count, gas estimate.

6. **Error classification** — The original handler distinguishes not-found,
   denied, invalid-input, and backend-failure. Ports often collapse these
   into generic errors. Map each to `petal::error` codes (-1, -2, -3, -4).
