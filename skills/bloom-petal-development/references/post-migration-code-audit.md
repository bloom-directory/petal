# Post-Migration Code Quality Audit

After a petal migration passes functional gap analysis and tests pass, a
systematic code quality pass removes dead code, eliminates duplication, and
catches subtle logic errors. Observed during the `bloom-petal-enso` migration
where the codebase compiled clean with 38 tests but had ~500 lines of dead
code and 8 code smells.

## When to run

After the functional gap analysis is closed and all tests pass — but **before**
committing the final version or declaring the migration complete.

## Method

### 1. Dead code audit (run `cargo check` first, then grep)

```sh
# Find unused pub fns in the API client
grep -n 'pub fn' src/api.rs | while read line; do
  fn=$(echo "$line" | sed 's/.*fn \([a-z_]*\).*/\1/')
  count=$(grep -rn "api::$fn\|$fn(" src/ files/ | grep -v "pub fn $fn" | wc -l)
  [ "$count" -eq 0 ] && echo "DEAD: $fn (0 callers)"
done

# Find unused Host trait methods
grep -n 'fn ' src/runtime.rs | grep -v '//' | while read line; do
  method=$(echo "$line" | sed 's/.*fn \([a-z_0-9]*\).*/\1/')
  count=$(grep -rn "\.$method(" src/ files/ | wc -l)
  [ "$count" -eq 0 ] && echo "DEAD: $method (0 callers)"
done

# Find unused struct fields
grep -rn 'session\.\|state\.' files/ src/ | grep -oP '\.\w+' | sort -u
# Cross-reference against struct definitions
```

Common dead code in petal migrations:

- **API client functions never called** — the original crate had multiple API
  endpoints (quote, simulate, validate, route) but the petal only uses one
  (route). The unused client functions and their request/response types
  survive the port because they were in the same module.
- **Host trait methods never called** — e.g. `erc20_symbol` was ported but
  never used because token symbols come from a static table, not on-chain
  reads.
- **Struct fields initialized to None/empty and never set** — e.g.
  `min_settlement_delta`, `source_tx_hashes` were declared, initialized, but
  never populated with meaningful data.
- **Helper functions with `_pub` suffix** — a wrapper that just calls a
  private function and adds nothing. Make the underlying function public.

### 2. Duplication audit

Look for logic copy-pasted across route files:

- **Policy aggregation** — iterating `policy_checks.as_array()` to compute an
  overall pass/warn/deny appears in both `policy_check.json` and
  `review_intent.json`. Extract as
  `Session::policy_overall() -> PolicyOutcome` — a tri-state is an enum, not a
  `&str` that every caller re-compares.
- **Simulation logic** — `create()` often has inline eth_call logic that
  duplicates `simulate_route()`. Extract a shared helper that takes a
  `RouteResponse` directly (see `defi-chain-patterns.md` § Simulation).
- **Outbox inspection** — iterating `intent_states` and calling `tx_inspect`
  per outbox ID appears in `settlement.json`, `outbox.json`, and `receipt.json`.
  Consider a shared helper that returns `Vec<OutboxState>`.

### 3. Logic audit

Check for redundant computations and unnecessary side effects:

- **Redundant route verification in confirm()** — if a guard 5 lines above
  already returns early when route verification fails, the `route_verified`
  variable at the policy re-eval point is always `true`. Replace with literal
  `true` and delete the dead computation.
- **Intermediate save before early return on error** — if the function
  returns `Err` after a policy deny, an intermediate `save()` call is
  wasted I/O. The session is in-memory and will be discarded on `Err`.
- **`match` reimplementing `unwrap_or`** —
  `match f() { Ok(v) => v, Err(_) => default }` → `f().unwrap_or(default)`.
- **`match` reimplementing `.ok()`** —
  `match f() { Ok(v) => Some(v), Err(_) => None }` → `f().ok()`.

### 4. Naming and clarity

- **Underscore-prefixed params that are actually used** — `_host_wallet_addr`
  implies unused, but if the function body reads it, drop the underscore.
- **Vec::remove(0) in a loop** — O(n) per removal. Use `drain(..1)` for
  front-of-vec trimming (or a VecDeque if frequent).

### 5. Clippy sweep

```sh
cargo clippy 2>&1 | grep 'warning:'
```

Fix every warning. Silencing one with `#[allow(...)]` is not a fix — a warning
is the compiler reporting that the code is wrong.

- **Mechanical warnings** — collapsible if, unnecessary casts, manual `.ok()`,
  `unwrap_or` reimplementations. Rewrite them.
- **Too many arguments** — group the arguments into a struct, or take the
  struct whose fields they already are. Not
  `#[allow(clippy::too_many_arguments)]`, and not a builder.

### 6. Verify

After all changes:

```sh
cargo check   # must pass
cargo test    # test count may decrease (dead code tests removed) but must pass
cargo clippy  # zero warnings, with no added #[allow(...)]
```

## Checklist

- [ ] All dead API client functions and their types removed
- [ ] All dead Host trait methods + their helper functions removed
- [ ] All dead struct fields removed (check all initializers + route files)
- [ ] Duplicated policy/simulation/outbox logic extracted to shared helpers
- [ ] No intermediate saves before error returns
- [ ] No redundant computations where an earlier guard already proved the value
- [ ] No underscore-prefixed params that are actually used
- [ ] Clippy and rustc clean, with no `#[allow(...)]` added to get there
- [ ] `cargo check` + `cargo test` pass
