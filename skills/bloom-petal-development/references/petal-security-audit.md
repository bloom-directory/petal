# Petal Security Audit — Checklist & Known Antipatterns

Reusable audit methodology for Bloom Petals. Use alongside the "Audit a petal"
section of SKILL.md.

## Dead-code verification (critical)

The single most valuable check. Petals often define security functions or
fields that *look* like active protections but are never wired up.

### What to grep

```sh
# Functions that sound like security checks — verify each has a call site
grep -rn 'fn.*\(verify\|check\|validate\|enforce\|contains\)' route/src/

# Fields that sound like verified flags — trace where they're SET
grep -rn '_verified\|_enforced\|_checked\|_confirmed' route/src/
```

For each hit:
1. Find the definition — is the function body non-trivial?
2. Find call sites — is it called from the hot path (create, confirm, etc.)?
3. For fields — is the value ever set to `true`/`Some(...)` at runtime, or is it
   hardcoded `false`/`None`?

## Opaque calldata trust

When a petal receives transaction calldata from an external API,
the calldata is an opaque hex blob. Check:
- Does the petal decode the calldata to verify any fields within it?
- Is the quoted output amount enforced as a minimum in the calldata, or is it
  display-only?
- Could a compromised API response redirect funds without detection?

## State machine typing

Stringly-typed state fields (`Session::state: String`, `IntentState::status:
String`) are a maintenance hazard. State values scattered as string literals
across modules compile fine with typos. Recommend enums with
`#[serde(rename_all = "snake_case")]`.

## Session lifecycle

- **No expiry**: sessions persist indefinitely. Check whether stale routes
  with outdated quotes can be confirmed hours later.
- **O(n) trim**: `Vec::drain(..1)` per trim is O(n). Recommend `VecDeque`.
- **Confirm lock**: check for proper TTL and stale-lock recovery.

## UX audit points

- **Input parser rigidity**: does it reject common synonyms ("for" vs "to",
  "trade" vs "swap", comma-formatted numbers)?
- **Token resolution errors**: does it list known symbols or suggest raw
  address format?
- **Misleading route names**: e.g. `wait_settlement.json` that returns
  immediately with a retry hint instead of blocking.
- **Error code consistency**: are different failure modes distinguishable by
  code, or do they share `-1`?
- **Missing bounds**: slippage, amount, or fee parameters without hard caps.

## Cross-module code duplication

When a petal has both a canonical route module and a legacy/compatibility module
(e.g. `api.rs` + `legacy.rs`), check for large-scale copy-paste: Host trait impl, fetch/submit
helpers, EIP-712 hashing, decimal parsing, phase logic, and test infrastructure.

Risks:
- A behavior fix applied to one module but not the other creates silent
  divergence (the two `decimal_units` implementations already differed slightly).
- Duplicated constants hardcoded in both modules
  must be updated in lockstep.

Recommend extracting shared code into a `common.rs` module.

## Legacy/compatibility route validation divergence (critical)

When a petal has backward-compatible routes alongside canonical ones, the
legacy routes often retain weaker validation than the new routes.

What to check:
- Compare validation depth between canonical and legacy/compat routes that
  handle the same upstream API.
- Grep for `validate_order` or equivalent structural validators — confirm
  every code path that accepts an upstream response calls them.
- Pay special attention to routes that were "kept working" during a refactor.

## Apparent dead state-machine values (CAUTION: check persisted state)

States matched in `match` arms but never *assigned* by current code may look
dead, but in a petal with durable store-backed state, those values can appear
in records written by a previous code version and read back at runtime. Removing
the match arms breaks backwards compatibility with persisted states on disk.



What to grep:
```sh
# For each phase/status string literal, confirm it appears on both sides
# of an assignment (=) and in a match arm
grep -rn '"[a-z_]*"' route/src/ | grep -v 'test\|assert\|format\|println'
```

Before flagging such a value as dead code, confirm:
1. Is there a store read that loads arbitrary phase strings from persisted state?
2. Do any tests assert the value's behavior? If so, it's backwards-compat, not dead.

## Signature non-persistence (positive check)

Verify that signature bytes are never persisted to the store. Confirm:
- Signature bytes are used for submission then dropped — not stored in state.
- Transport error messages during submission are replaced with opaque constants.
- Tests assert no signature/secret string appears in persisted state.

## Module structure

- Modules over ~800 lines should be split by responsibility.
- Duplicated utility functions across modules (e.g. two `normalize_decimal`
  implementations) should be extracted to shared utils.
- Hand-rolled implementations (hex→decimal, ABI encoding) where existing crate
  dependencies (alloy, hex) already suffice.

## Silent chain-read error swallowing

When a route handler performs multiple independent chain reads (e.g.
`current_tree_size`, `latest_asp_root`, `pool_scope`) and uses
`.ok()` / `.unwrap_or_default()` / `Option::map(...).unwrap_or_default()`
on any of them, a failed RPC call silently produces an empty or zero
value in the response JSON. The handler returns HTTP 200 with incomplete
data — no error surfaced to the caller.

### How to detect

Grep for `.unwrap_or_default()` and `.unwrap_or("")` in route handlers
and shared code that constructs response JSON:
```sh
grep -rn 'unwrap_or_default\|unwrap_or("")' route/files/ route/src/
```
For each hit, trace whether the `Option` came from a fallible chain/HTTP
read (`.ok()`) — if so, the error is being silently discarded.

### General rule

When a handler performs N independent reads and only gates on a subset,
the ungated reads silently degrade the response. Either gate on all of
them (fail the whole response if any critical read fails) or explicitly
surface per-field errors. Never let a failed read become an empty string
or zero in a 200 response.

## Severity ranking guide

- **CRITICAL**: active security gap that could lead to fund loss or secret
  leakage in production (dead verification code, unenforced slippage).
- **HIGH**: correctness bug or architectural limitation affecting real usage
  (no session expiry, unattributable cross-chain settlement).
- **MEDIUM**: UX friction that degrades agent/user experience (rigid parser,
  unhelpful errors, misleading names).
- **LOW**: code smell or maintenance hazard with no immediate user impact
  (stringly-typed states, oversized modules, duplication).

