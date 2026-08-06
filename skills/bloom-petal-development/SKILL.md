---
name: bloom-petal-development
description: Build, migrate, refactor, review, and validate Bloom Petals that expose domain behavior as route-based virtual files through the Petal SDK and builder. Use when turning an API or workflow into a Petal, creating a Petal, adding or changing route files, designing route and shared-code boundaries, declaring capabilities and manifest policy, investigating oversized route components, or preparing a Petal for packaging and release.
version: 0.1.0
---

# Bloom Petal Development

Build Petals as explicit virtual files whose behavior is understandable from
their route source. Keep endpoint policy local and share only substantial,
typed infrastructure.

## Authoring principle: general patterns only

This skill documents patterns that apply to **all** petals. Do not embed
references, anecdotes, domain knowledge, or file paths specific to a
particular petal (gasless, venice, relay, polymarket, etc.). If a pattern
was learned while working on a specific petal, generalize it before adding
it here — remove the petal name, replace concrete field names with generic
placeholders, and keep only the transferable lesson.

Petal-specific domain knowledge (API quirks, field-value maps, integration
narratives) belongs in session memory or a separate petal-specific
reference — never in this skill.

## Establish the local contract

1. Read the target repository's `AGENTS.md`, `README.md`,
   `petal-build.toml`, manifests, and build scripts before editing.
2. Inspect the exact route files and shared functions in scope.
3. Treat the target repository's pinned Petal SDK and tooling as authoritative.
   Do not assume another Petal uses the same SDK revision or helper API.
4. Preserve unrelated work in a dirty worktree.

When starting a new Petal, prefer the canonical `petal new` scaffold and adapt
its generated structure instead of inventing a parallel layout:

```sh
petal new <petal-name> <destination>
```

The command refuses an existing destination. Do not replace an existing
repository with a fresh scaffold.

## Resolve the product and trust contract

Before coding, write down or recover these decisions from the user's request,
the target repository, and its domain/API documentation:

- the users and tasks the VFS should support;
- the proposed route tree and the content or body schema of every file;
- which reads contact an upstream, which writes have side effects, and which
  environments or networks are allowed;
- the lifecycle for every durable operation, including retry, approval,
  failure, recovery, and completion states;
- which credentials, wallet operations, settings, and stored values cross a
  trust boundary;
- the pinned Petal contract, SDK, CLI, and release target.

Do not invent a production write flow, credential format, signing intent, or
claim of completion. If one of those is genuinely unspecified, make the
read-only or sandbox choice, state the assumption, and ask only for decisions
that would materially change the route tree or trust boundary.

Use a compact route contract while designing:

| Route | Kind | Parameters | Read | Write body/effect | State | Capabilities |
| --- | --- | --- | --- | --- | --- | --- |
| `markets/[slug]/book.json` | file | `slug` | projected order book | — | none | `bloom:http` |

## Design the virtual filesystem first

Define each route's path, parameters, file kind, read/write behavior,
capabilities, cache behavior, and durable inspection surface before
implementing it. Prefer small files with one clear responsibility.

For each route file, keep these decisions local:

- route-parameter parsing and validation;
- upstream endpoint and request-body selection;
- response projection or user-facing read content;
- write-body parsing and route/action compatibility;
- the route's read description when a writable file is also readable;
- route-specific errors and semantics.

Use the route macro's local directory, `read`, or `read` plus `write` shape.
A writable route must define both handlers in that route file; use its read
handler for safe instructions or current non-secret state.

## Map route files explicitly

The configured `routes` directory is executable source, not a conventional Rust
module tree. Every `.rs` file below it becomes a separately compiled component:

- `route/files/$index.rs` defines the root directory;
- `route/files/markets/$index.rs` defines the `markets` directory;
- `route/files/markets/[slug]/book.json.rs` defines
  `markets/<slug>/book.json`;
- a bracketed segment such as `[slug]` is available through
  `petal::param(ctx, "slug")`;
- `$list.rs` is unsupported; use `$index.rs`.

Directory contents are explicit. Use `list` for a fixed child list,
`fallible_list` for a computed result that does not need the context, and
`ctx_list` for a context-dependent result. Use `petal::dir`, `petal::file`, or
`petal::writable` to report the correct child kind. Listing a dynamic directory
does not happen automatically.

Validate parameters for their domain before using them in URLs, storage keys,
or VFS paths. Use `petal::is_safe_segment` when a value becomes a path segment;
do not assume supplied context parameters are safe merely because the source
filename uses brackets.

## Select the local handler and spec

Use one supported `petal::route_file!` shape per route:

- directory: `list`, `fallible_list`, or `ctx_list`;
- read-only file: `read`;
- writable file: `read` plus `write`.

Start with the narrowest matching spec:

| Spec | Intended use | Default declared capabilities |
| --- | --- | --- |
| `static_dir_spec()` | fixed directory | none |
| `store_dir_spec()` | stored/dynamic directory | store, VFS read |
| `http_dir_spec()` | upstream-backed directory | HTTP |
| `static_read_spec()` | pure/static file | none |
| `http_read_spec(ttl_ms)` | cacheable upstream read | HTTP |
| `store_read_spec()` | stored read | store |
| `wallet_http_read_spec(ttl_ms)` | HTTP read using wallet VFS data | HTTP, VFS read |
| `account_read_spec()` | HTTP/store/wallet account read | HTTP, store, VFS read |
| `chain_read_spec()` | side-effecting, uncached chain flow | broad host set |
| `write_spec()` | uncached writable file | broad host set |
| `signing_write_spec(intent)` | writable file with signing intent | broad host set |

Override with `.caps(&[...])` when the compiled route imports a narrower or
different set. The broad write and chain specs are convenience defaults, not
permission to retain unused capabilities.

Return errors through `petal::error`: `-1` not found, `-2` denied, `-3`
invalid input, and `-4` backend failure. Other codes become unsupported.
Sanitize upstream and storage errors before returning them.

## Avoid path-based dispatch

Do not create catch-all `read`, `write`, or `dispatch` functions that inspect
route identity. This covers every accessor the SDK exposes now or later,
including `current_route_path` and `current_route_canonical_path`, as well as
filenames, path suffixes, and parameter presence. Do not route through chains
of `contains`, `ends_with`, or string-keyed matches.

The structural guarantee is that each route file is generated as its own
component crate and the `route_file!` macro wires exactly one local handler
per file, so shared code cannot serve a route unless that route opts in. Treat
this as the primary enforcement. The toolchain's `petal check` reconciles a
route's declared capabilities against its compiled imports, but it does not
inspect source for route-identity access, so the path-dispatch rule has no
canonical lint: a repository-level source check (for example a grep over the
shared crate for `current_route_path` and `current_route_canonical_path`) is
the authoritative gate for this rule. Keep its denylist aligned with the
accessors the pinned SDK actually exposes.

Avoid:

```rust
pub fn read(ctx: &petal::Ctx) -> petal::DispatchResponse {
    let path = petal::current_route_path(ctx);
    if path.ends_with("status.json") {
        // ...
    } else if path.contains("orders/") {
        // ...
    } else {
        // ...
    }
}
```

Prefer:

```rust
petal::route_file!(
    spec: petal::http_read_spec(5_000),
    read: |ctx: &petal::Ctx| {
        let market = match petal::param(ctx, "market") {
            Ok(market) => market,
            Err(response) => return response,
        };
        read_market_status(market)
    },
);
```

This keeps route behavior discoverable and allows each independently compiled
component to discard code it does not use.

## Share typed infrastructure

Put code in the shared route crate only when it represents substantial,
reusable behavior rather than endpoint selection. Good shared candidates
include:

- protocol types, serialization, hashing, and signing;
- bounded HTTP, store, chain, transaction, and VFS adapters;
- idempotency and durable state machinery;
- domain validation or policy used identically by multiple routes;
- multi-step operations with typed inputs and outputs.

### Host trait pattern for testability

When a petal makes HTTP calls, store reads/writes, signing, or time checks,
abstract them behind a `pub(crate) trait Host` so business logic can be
unit-tested with a `MockHost` without touching real network, store, or keys.
This is the canonical way to achieve coverage on multi-step flows
(auth → payment → API call → persist):

```rust
// common.rs
pub(crate) trait Host {
    fn store_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String>;
    fn store_put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), String>;
    fn http_fetch(&mut self, request: &HttpRequest) -> Result<HttpResponse, String>;
    fn sign_hash(&mut self, hash: &[u8; 32]) -> Result<[u8; 65], String>;
    fn now_ms(&mut self) -> Result<u64, String>;
}

// Production impl — delegates to petal::sdk functions
pub(crate) struct BloomHost;
impl Host for BloomHost { /* calls petal::sdk::store_get, etc. */ }

// Test impl — in-memory, queue-based
#[cfg(test)]
pub(crate) mod test_helpers {
    pub(crate) struct MockHost {
        pub store: HashMap<String, Vec<u8>>,
        pub http_responses: VecDeque<HttpResponse>,
        pub sign_results: VecDeque<Result<[u8; 65], String>>,
        pub sign_requests: Vec<[u8; 32]>,
        pub now_ms_value: u64,
    }
    impl Host for MockHost { /* pops from queues, records calls */ }
}
```

Key design points:
- `MockHost` uses `VecDeque` for HTTP and sign results so tests can queue
  multi-step responses (e.g. 402 then 200 for a payment retry flow).
- `sign_requests` records every hash passed to `sign_hash` so tests assert
  call counts and inspect signing behavior.
- The production `BloomHost` struct has no fields — it's a zero-type that
  delegates entirely to `petal::sdk::*`.
- All business-logic functions take `&mut impl Host` as their first argument.
  Route handler entry points create `common::BloomHost` and pass it through.

Pass typed values from the route file into shared functions. Avoid passing a
route path or filename and asking shared code to infer the operation. A small
amount of repetition in route files is preferable to hidden coupling through
dynamic dispatch.

## Declare least capabilities

Keep three scopes distinct:

- the route spec declares the component's `required_caps`;
- `[caps].allowed` in `petal.toml` declares the package-wide ceiling;
- `[[net.allow]]`, `[sign]`, and `[store]` constrain particular host surfaces.

Make each route's metadata capabilities the authority it intentionally needs
at runtime, within the ceiling inferred from its compiled imports. Make the
package ceiling the union of capabilities the Petal intentionally uses.
Refactoring shared dispatch into local handlers often removes imports through
dead-code elimination, so revisit both scopes after architectural changes.

Use the narrowest applicable route spec, then override capabilities only when
the route needs a different set. It is valid for route metadata to narrow the
compiled import ceiling; Bloom grants only the narrowed set, so any omitted
host surface must be unreachable during successful execution.

Know the validator boundaries:

- Petal CLI `build`, `check`, and `package` reject a capability declared by a
  route spec when the compiled component does not import it, and reject unknown
  component imports.
- Petal CLI packaging validates only basic `petal.toml` identity; it does not
  prove that the complete manifest policy covers compiled imports.
- Bloom `petals build` and install validation reject compiled imports not
  covered by `[caps].allowed` and the applicable network, signing, and store
  policy. They also enforce route metadata as a runtime narrowing boundary.

Inspect the top-level `bloom:*` imports of changed route components with
`wasm-tools component wit`, then compare them with the route spec and package
manifest. Treat unexpected imports or authority as architecture feedback, not
as a reason to broaden declarations.

Import-to-capability mapping is:

- `bloom:http/fetch` → `bloom:http`;
- `bloom:store/kv` → `bloom:store`;
- `bloom:sign/signing` → `bloom:sign`;
- `bloom:tx/outbox` → `bloom:tx.outbox`;
- `bloom:chain/read` → `bloom:chain`;
- `bloom:vfs/readwrite` → VFS read and/or write according to used functions;
- `bloom:env/runtime` → no declared capability.

The Petal CLI currently treats a VFS interface import as both VFS capabilities
when checking a route's declared ceiling; Bloom's package validator performs
the finer used-function inference. Confirm the final policy with Bloom rather
than inferring it from the interface name alone.

Network egress is part of least capability. Declare every reachable upstream
host in `petal.toml` with a separate `[[net.allow]]` table, explicit methods,
and path prefixes; never rely on a wildcard host. Shared HTTP helpers should
resolve targets from a fixed, enumerated source (for example a `Network`
variant that returns a `&'static str` URL) so route code cannot direct a
request at an arbitrary host. If a new upstream is introduced, widen
`[net.allow]` deliberately, not as a side effect of editing a helper.

## Preserve write semantics

For side-effecting routes:

- validate body size and shape before external work;
- bind the body to the route's permitted operation;
- preserve stable idempotency identifiers across approval retries;
- store enough status, response, and error state for inspection;
- distinguish approval, staging, submission, broadcast, confirmation, and
  domain completion;
- never report a broadcast, confirmation, or fill solely because a route write
  was accepted.

Do not exercise production writes or material funds unless the user explicitly
authorizes that environment and action.

## Protect sensitive material

Private keys, derived agent keys, API secrets, bearer tokens, secret-store
values, and replayable or not-yet-submitted signatures are confidential.
Store them only in the store's secret namespace and never copy their bytes
into the public state namespace, errors, logs, or a read response. Treat
signature bytes as confidential unless the product contract explicitly
classifies a finalized signature as public. When a read needs to expose signing
state, return only what an auditor needs: status, intent, outcome, and
non-secret identifiers.

When a route signs locally with a stored key, the route's declared capabilities
should omit the host signing capability and the signing function must live
behind a typed helper that cannot return key bytes to its caller. Treat the
boundary between the secret namespace and the readable VFS as a hard wall and
add tests that confirm public routes and packaged artifacts contain no secret
value. Use the pinned SDK or host binding for secret reads; do not assume
`petal::sdk::store_get` reads the secret namespace.

## Validate in layers

Use the repository's pinned commands and scripts. At minimum:

1. Format and run host-side unit tests.
2. Run clippy or the repository's equivalent static checks.
3. Run the repository's route-architecture source check.
4. Run `petal build --root <root>` to compile every route component.
5. Run `petal check --root <root>` against the just-built components.
6. Inspect changed components' imports and compare all three policy scopes.
7. Run `petal package --root <root> --out <versioned>.petal.tar.gz` or the
   repository's release-validation command. Packaging refuses to overwrite an
   existing archive.
8. Run the pinned Bloom package-validation command, normally
   `bloom petals build <root>`, and exercise the target repository's VFS smoke
   tests when available.
9. Confirm the expected route count and inspect unexpected component-size
   changes.

Do not use a successful host-crate build as evidence that route sources compile;
route files are independently generated component crates. `petal check` does
not rebuild, so do not use it against stale artifacts. Packaging checks built
routes but does not fully validate `petal.toml` policy and does not replace
route-level tests or a runtime VFS smoke test.

<<<<<<< ours
=======
## Pitfall: petal CLI is vendored per-repo, not system-installed

The `petal` binary is not on `$PATH`. Each petal repo vendors it at
`./target/petal-tool/bin/petal`. The validation steps above that say
`petal build` / `petal check` should use either:

- the repo's own scripts (`scripts/build.sh` invokes the correct binary
  automatically), or
- the explicit vendored path: `./target/petal-tool/bin/petal check --root .`

Running `petal` bare will produce `command not found`.

## Pitfall: `?` operator in DispatchResponse functions

Route handler functions return `petal::DispatchResponse`, not `Result<T, E>`.
The `?` operator cannot be used inside them. When refactoring shared helpers
that return `Result<T, DispatchResponse>` (e.g. `validate_request`,
`prepare_data`), call them with explicit `match` or `if let` rather than `?`:

```rust
// WRONG — won't compile, the handler returns DispatchResponse, not Result
common::validate_request(&response)?;

// RIGHT
common::validate_request(&response)?;
// ...but only inside a helper that already returns Result.
// In the top-level handler, use:
if let Err(response) = common::validate_request(&response) {
    return response;
}
```

This pitfall recurs when extracting shared validation logic into a `common.rs`
module — the extracted functions return `Result<_, DispatchResponse>` for
composability, but the call sites in route handlers can't use `?`.

## Pitfall: `.gitignore` must cover nested `route/target/`

Petal repos have `Cargo.toml` in `route/`, not the repo root. The default
scaffold `.gitignore` only has `/target`, but the actual build artifacts land
in `route/target/`. Without adding `/route/target` to `.gitignore`, a
`git add -A` stages **thousands** of build artifacts.

Always verify the `.gitignore` covers the actual target directory:

```gitignore
/target
/route/target
*.wasm
```

After staging, check the file count — it should be under ~30, not thousands:

```sh
git add -A && git status -s | wc -l
```

## Pitfall: base64 0.22+ requires `Engine` trait import

The `base64` crate moved `.encode()` / `.decode()` behind the `Engine` trait
in 0.22. Even when calling `base64::engine::general_purpose::STANDARD.encode()`,
you must bring the trait into scope:

```rust
use base64::Engine; // required for .encode() / .decode()
```

Without it: `error[E0599]: no method named 'encode' found for struct
'base64::engine::GeneralPurpose'`.

## Pitfall: `pub use` re-exports cannot reference `pub(crate)` items (E0364)

When refactoring internal functions from `pub` to `pub(crate)` for better
encapsulation, `lib.rs` re-exports that reference them fail:

```
error[E0364]: `check_balance` is only public within the crate, and cannot
be re-exported outside
```

Only `pub` items can appear in `pub use` statements. Either keep the function
`pub` if it's part of the crate's public API, or remove it from the re-export
and call it as `crate::module::function()` from `lib.rs` wrapper functions.

## Pitfall: `DispatchResponse` import across modules

When splitting petal logic across multiple source modules (`common.rs`,
`api.rs`, `handlers.rs`, etc.), `petal::DispatchResponse` cannot
be imported via `use petal::{... DispatchResponse ...}` in `common.rs` AND
then re-imported from `crate::common` in sibling modules without triggering
`E0252` (name defined multiple times) or `E0603` (private import).

The fix: re-export it once from `common.rs` and import from there everywhere:

```rust
// common.rs — re-export once
use petal::{HostStatus, HttpRequest, HttpResponse, SdkError, SignHashOutcome, SignRequest};
pub(crate) use petal::DispatchResponse;
```

```rust
// api.rs, handlers.rs, etc. — import from common, not petal
use crate::common::{self, DispatchResponse, Host, ...};
```

Do NOT include `DispatchResponse` in both the `use petal::{...}` block and
a `pub(crate) use` in the same file — the compiler sees two definitions.

## Pitfall: integration tests can't see `pub(crate)` items

Integration tests (`route/tests/*.rs`) compile as external crates and can
only access the crate's **public** API (`pub` items). Any `pub(crate)`
function, trait, struct, or constant is invisible.

This means flow-level tests that need `MockHost`, `Host` trait methods,
`pub(crate)` parsing functions, or internal types **must live as `#[cfg(test)]
mod tests` inside the source module**, not in `tests/`.

Put in `tests/` only: public API round-trip tests, type serialization,
request parsing via re-exported `pub` functions.

## Pitfall: clippy let-chain suggestions don't compile under pinned edition

When refactoring shared code (e.g. into `common.rs`), clippy may suggest
collapsing nested `if let ... { if ... }` blocks into let-chains
(`if let Some(x) = ... && x.is_foo()`). Petal crates often pin a pre-2024
Rust edition in `Cargo.toml`, so let-chains are rejected:

```
error: let chains are only allowed in Rust 2024 or later
```

When this happens, extract the inner condition into a `let` binding before
the `if`:

```rust
// WRONG — let-chain, edition-gated:
if let Some(output) = order.get("output")
    && output.get("calls").and_then(Value::as_array).is_some_and(|c| !c.is_empty())
{ ... }

// RIGHT — works on any edition:
let has_dest_calls = order
    .get("output")
    .and_then(|o| o.get("calls"))
    .and_then(Value::as_array)
    .is_some_and(|c| !c.is_empty());
if has_dest_calls { ... }
```

For simple `if let Some(x) = ... && predicate(x)` cases, `.is_some_and()`
is a clean one-liner alternative that works on any edition:

```rust
// Also works on any edition:
if request.temperature.is_some_and(|temp| !(0.0..=2.0).contains(&temp)) {
    return Err(invalid("temperature out of range"));
}
```

Run `cargo clippy --all-targets` from `route/` as the final gate before
committing — it catches collapsible-if, unused import, and dead-code
warnings that `cargo test` does not.

**Recurring clippy pedantic lints in petal code:**
- `manual_ignore_case_cmp`: use `a.eq_ignore_ascii_case(b)` instead of
  `a.to_ascii_lowercase() == b.to_ascii_lowercase()`. Common when comparing
  EVM addresses or token identifiers.
- Import ordering: `serde_json::{json, Value}` not `{Value, json}` —
  `cargo fmt` fixes this automatically, but pre-formatting writes trigger it.
- `needless_borrows_for_generic_args`: pass `&value` not `&&value` to
  generic functions that take `&T`.

## Pitfall: automated dedup scripts stripping backwards-compat branches

When bulk-removing duplicate functions with a Python/sed script (e.g. extracting
shared code from two modules into `common.rs`), the script may
also remove context-specific branches that look identical but exist for
backwards compatibility with persisted disk state. A function may include
an extra status variant in one module that the other doesn't have
— a script that took the "common subset" dropped it and broke a test.

**Rule: after any automated dedup, run the full test suite immediately.** Do
not batch the dedup with other changes. If a test fails, diff the removed code
against the original to find the behavioral divergence, and restore the
module-specific branch rather than modifying the test.

When asked to audit or review a petal, work through these phases
systematically and severity-rank findings (CRITICAL / HIGH / MEDIUM / LOW).
When implementing audit fixes, investigate each finding until certain before
making the change. If a finding turns out to be wrong (e.g. "dead code" that is
actually backwards-compat), correct the audit conclusion and document why — do
not blindly implement the original recommendation. The user values autonomous
execution: iterate to completion on a feature branch without stopping for
confirmation at every step.

See `references/petal-security-audit.md` for the full checklist, known
antipatterns catalog (with grep recipes), and severity-ranking guide.

### Phase 1 — Locate and map the codebase

**Find the repo first.** Each Bloom petal typically lives in its own dedicated
repo under the `bloom-directory` GitHub org (`bloom-petal-<name>`). Before
grepping the local filesystem, list the org's repos to confirm the target
exists and clone it if needed:

```sh
curl -s "https://api.github.com/orgs/bloom-directory/repos?per_page=100" \
  | python3 -c "import json,sys; [print(r['name']) for r in json.load(sys.stdin)]"
```

The user may refer to a petal by a product name that differs from its repo name
or any string in the source (e.g. a product name that differs from the repo name). If no
repo matches, ask the user where it lives rather than auditing a best-guess
alternative.

**Then map it.** Read every `.rs` file in `route/src/` and `route/files/`, the
manifests (`petal.toml`, `petal-build.toml`, `Cargo.toml`), and all integration
tests. Run `git diff main...HEAD --stat` to identify what the current branch
changed. Compile tests with `cargo test --no-run` from the `route/` directory
(not the repo root — `Cargo.toml` lives in `route/`).

### Phase 2 — Verify security wiring (most important)

The highest-value audit technique: **confirm that security-critical functions
are actually called and security-critical fields are actually set at runtime**.
Petals often define a verification function or a `_verified` boolean field that
reads as "this check exists" but is dead code — never invoked, or hardcoded to
`false`/`None`. The function name and type signature create an illusion of
safety.

Concretely:
- grep for every function whose name contains `verify`, `check`, `validate`,
  `enforce`, `contains` — confirm each has at least one call site;
- grep for every struct field ending in `_verified`, `_enforced`, `_checked`
  — trace where it's set and confirm it's not always `false`/`None`;
- for opaque calldata (e.g. from an external API), check whether the petal
  decodes or cryptographically verifies any fields within it, or blindly
  trusts the API response.

### Phase 3 — UX surfaces

Review every route file's read description, error messages, and write-body
parsing. Check:
- input parser flexibility and error message helpfulness;
- misleading route names (e.g. `wait_settlement` that doesn't block);
- unhelpful error codes (same code for different failure modes);
- missing validation bounds (e.g. slippage with no upper cap).

### Phase 4 — Code smells and bugs

Check for stringly-typed state machines (should be enums), oversized modules,
duplicated utility functions across modules, hand-rolled implementations where
existing crate dependencies suffice, and session lifecycle gaps (no expiry,
no staleness protection, O(n) operations in hot paths).

### Phase 5 — Live API validation

Static analysis (Phases 1–4) verifies the code handles what you *think* the
upstream API returns. It does not verify what the API *actually* returns.
For petals that proxy an external API, hit the
upstream's live endpoint with a representative request and verify:

1. **Response shape matches code expectations** — every field the petal
   reads from the API response is present and has the expected type.
2. **Undocumented fields behave as assumed** — fields the petal depends on
   but that aren't in the upstream's docs should be tested with
   multiple routing variants to
   confirm actual values.
3. **Routing variants produce expected step structures** — test same-chain
   vs cross-chain, different currency pairs, and edge cases the unit tests
   mock but never exercise against the real API.
4. **Quote-only paths return usable data without state** — verify that a
   read-only request returns enough information for the caller to make a
   decision before committing to a write.

Use only read-only upstream endpoints (quote, estimate, status). Never
submit transactions, permits, or orders during a review.

Create a standalone validation script (`scripts/live-quote-check.sh` or
similar) that exercises these paths and can be run in CI or before
deployment.

Static analysis only verifies the code handles what you *think* the API
returns. Live testing often reveals undocumented field values, missing
parameters, or routing behaviors that are impossible to discover through
code review alone. Unit tests with `MockHost` only verify the code handles
what the tests *pretend* the API returns.

### Phase 6 — VFS wiring and genericity

Two concerns that Phases 1–5 don't explicitly cover. Run them when the user
asks "is the VFS fully functional?" or "make sure this petal is generic."

**VFS wiring completeness.** Verify systematically:

- **Store key schemes** — trace every `store_get` / `store_put` /
  `store_put_new` call and confirm the key pattern is consistent between
  save and load for each state type. Different modules should not produce
  colliding keys.
- **Cross-petal VFS reads** — `vfs_read` calls (e.g.
  `wallets/{wallet}/address`) resolve from another petal's namespace. The
  code is verifiable; whether the namespace resolves at runtime depends on
  deployment topology — state this gap explicitly rather than asserting
  it works.
- **Directory listings** — `$index.rs` files that return `Vec::new()` make
  child routes unreachable through directory traversal. Clients must know
  exact IDs. This may be intentional (the SDK may not expose `store_list`),
  but it is a product gap worth flagging: no transaction-history view is
  possible without an external indexer.
- **Idempotent retry** — confirm `load` is called before `save` / `create`
  on every entry point, so a retried request with the same `{wallet}/{id}`
  resumes from persisted state rather than duplicating or overwriting.

**Genericity assessment.** When a petal has both a generic and a
chain-specific (legacy) module, verify the generic route actually handles
every case the legacy module covers:

1. grep the generic module for chain-specific constants (chain IDs, token
   addresses, hardcoded decimals, chain names). Expect zero matches.
2. Trace which validator gates each request field. If the origin currency
   uses strict EVM address validation (42-char hex) but the destination
   uses a more permissive validator, the generic route can target chains
   that use non-standard address formats.
3. Construct a concrete request that passes the legacy module's hardcoded
   values through the generic route's typed request structs and validators.
   If it passes, the legacy module is a convenience wrapper, not a
   capability the generic route lacks.

### Assessing legacy code removal

When the user asks whether to remove legacy / compatibility modules
(especially for a v1):

1. **Verify the generic route covers every case** (see genericity
   assessment above). If the generic validators accept all values the
   legacy module hardcodes, removal loses zero functionality.
2. **Check for persisted VFS states** — if the petal has been deployed,
   legacy states may exist under the legacy key scheme. Removing the
   legacy module breaks status reads for those states unless a migration
   path is added.
3. **Calculate the reduction** — LOC deleted, route files eliminated,
   signing intents simplified, state machines removed. This is the
   payoff.
4. **Confirm deployment status** — if it is genuinely the first version
   with no deployed clients or persisted states, removal is clean with no
   migration needed.
5. **After deletion, check for orphaned shared functions.** When a
   module is removed, functions in `common.rs` that were only called by
   that module become dead code. The compiler will warn (`function is
   never used`). grep for each shared function's call sites before
   deleting — if the remaining module has its own inline version (a
   common pattern when the modules diverged), the shared copy can be
   safely deleted. If the remaining module does call it, keep it.

## Implementing audit fixes

After completing the audit and severity-ranking findings, implement
fixes on a feature branch (never `main`). The user values autonomous
execution: iterate to completion without stopping for confirmation at
every step.

### Workflow

1. **Create a feature branch** from `main` (e.g. `audit-fixes`).
2. **Classify each finding** before implementing:
   - **Confirmed** — implement as described.
   - **Reclassified** — the audit conclusion was wrong; document why and
     implement the corrected fix (e.g. "dead code" that is actually
     backwards-compat for persisted state).
   - **Dropped** — too invasive to wire safely without breaking the
     existing architecture (e.g. string→enum migration across dozens of
     call sites). Document the rationale.
3. **Implement security fixes first** (CRITICAL/HIGH), then UX (MEDIUM),
   then code quality (LOW).
4. **After each major change, run `cargo test`** from `route/` — do not
   batch unrelated changes before testing.
5. **For bulk dedup**, use a Python script to find and remove duplicate
   functions, but **run tests immediately after** to catch
   backwards-compat regressions (see pitfall above).
6. **Fix all clippy warnings** as the final step — `cargo clippy
   --all-targets` from `route/`.
7. **Run the architecture check**: `bash scripts/check-route-architecture.sh`
   from the repo root.
8. **Commit with a detailed message** listing each finding ID, severity,
   and what was done.
9. **Push the branch** and provide the PR creation URL. `gh` CLI may not
   be authenticated — use the GitHub web URL from the push output.

### Common fix patterns

- **Missing validation in legacy/compat routes**: port the canonical
  module's validator into shared code (`common.rs`) and call it from
  both modules. Return `Result<(), DispatchResponse>` from the shared
  validator for composability — but remember the `?` operator pitfall
  (call sites in `DispatchResponse` functions need `match` or `if let`).
- **`.unwrap()` on serialization**: replace with `match` + early return
  of a `backend()` error response.
- **Path-traversal safety**: add `is_safe_segment()` guards to every
  public entry point that accepts user-supplied wallet/id parameters.
- **Unrecognized upstream statuses**: surface the raw status string in
  the fallback response as `*_raw` for debugging, without trusting it.
- **Input charset tightening**: when restricting validator charsets,
  verify against real-world data. Domain names, token symbols, and chain
  identifiers may contain characters (parentheses, spaces) that a naive
  charset would reject.
- **Not-found UX**: return an informative `not_created` JSON with a
  write template instead of a bare error code.
- **Silent chain-read error swallowing**: when a handler does multiple
  independent chain reads, don't use `.unwrap_or_default()` on the
  results — a failed RPC silently produces empty values in a 200
  response. Surface per-field errors as `{"error": "..."}` JSON objects.
  See `references/petal-security-audit.md` → "Silent chain-read error
  swallowing" for detection grep and fix pattern.

## Register a petal in bloom core

Once a petal passes validation, packaging, and review, register it as a
default preinstalled petal. For dynamically-loaded WASM petals (the
common case), three integration points must be updated:

1. **Catalog entry** — `bloom/src/github_source.rs`:
   `PreinstalledPetal` const (name, repo, commit, release_tag, archive,
   expected_hash) + match arm in `preinstalled_petal()`. **This is the
   most important step** — without it `bloom init` cannot download the petal.
2. **Default list** — `bloom-proto/src/config.rs`:
   `default_preinstalled_petals()` + validation match in `validate()`.
3. **Test assertions** — update every test that asserts the default
   preinstalled list.

Dynamically-loaded petals do NOT need `PETAL_ID_*` constants or
`placeholder_digest_for()` entries — their `petal:<name>` identity is
handled generically by `is_petal_petal_id()`. Signing intents declared
in `petal.toml` are enforced at runtime through grant terms.

A GitHub release (with `petal-release.json`, archive, and `SHA256SUMS`)\nmust be published before `bloom init` can download the petal. See
`references/petal-release-publishing.md` for the exact artifact formats,
creation sequence, and a reusable release script template.

Then smoke-test: build, install into bloom, start daemon, verify the
mount appears, exercise read-only routes, and run quote-only flows.
For write-flow testing without signing, create a watch-only wallet
to exercise quote validation and state persistence up to the
`approval_required` gate. Fund-moving tests require separate explicit
authorization.

See `references/vfs-smoke-testing.md` for the full end-to-end smoke
test reference (release-artifact validation, watch-only wallet writes,
state field verification, multi-chain checks).

See `references/bloom-core-integration.md` for the full file map,
code examples, provenance-check pitfall, and pin-update sequence.

## Update bloom core pins after significant petal changes

When a registered petal undergoes structural changes (routes added or
removed, modules deleted, signing intents changed), the package hash
changes and bloom core's catalog pin becomes stale. `bloom init` will
reject the petal with a hash mismatch. After any structural change:

1. **Rebuild and repackage** the petal to get the new `package_hash`.
2. **Update both fields** in `github_source.rs`: the release commit
   constant (new HEAD SHA) and the `expected_hash` in the
   `PreinstalledPetal` struct.
3. **Update `AGENTS.md`** — deleted route files leave dangling references
   that confuse the next agent or reviewer. Check every route path
   mentioned in `AGENTS.md` against the actual `route/files/` tree.
4. **Check `petal.toml`** — removed signing intents (e.g. deleted route
   operations) must be removed from
   `[sign].allowed_intents`.
5. **Verify determinism** by rebuilding the package a second time and
   comparing the hash.
6. **Run bloom core tests** (`cargo test -p bloom -- --test-threads=1`)
   to confirm the new pins don't break existing assertions.
7. **Commit and push both repos** — the petal branch and the bloom core
   feature branch.

### Pitfall: bloom core tests fail under parallel build scripts

`cargo test -p bloom` with default thread count may fail with
`Text file busy (os error 26)` on tests that invoke `scripts/build.sh`
concurrently. This is a pre-existing race in the test fixtures, not a
real failure. Use `-- --test-threads=1` for reliable results.

## Write agent-facing documentation

Petal docs serve two audiences: human developers and autonomous agents.
An agent encountering the petal cold — no prior context, no conversation
history — must be able to discover supported routes, construct valid
writes, and navigate the full transaction lifecycle without guessing.
This is not optional polish; it is a correctness requirement.

### Three documentation surfaces

1. **`status.json`** — the machine-readable entry point. An agent reads
   this first. It must include:
   - One-line description of what the petal does
   - Supported origin tokens with chain IDs, addresses, decimals, and
     permit domains (or equivalent identity metadata) as a structured array
   - Notes on destination flexibility (e.g. "any supported chain")
   - Pointers to `README.md` and `AGENTS.md`

2. **`README.md`** — the complete reference. Must include:
   - Route table (path, method, purpose)
   - Token/chain reference table (what works, what doesn't, why)
   - Write body reference with field-by-field semantics
   - Concrete examples for each operation type (same-chain, cross-chain,
     non-EVM destinations)
   - Guidance on caller-chosen parameters (e.g. `minimum_output` heuristics)
   - Full transaction lifecycle with all statuses enumerated
   - Scope boundaries (what the petal does and does not support)

3. **`AGENTS.md`** — the operating contract for agents. Must include:
   - ASCII state machine diagram showing all statuses and transitions
   - Next-action table: for each status, what the agent should do next
   - Error recovery table: common error messages, causes, and resolutions
   - Safety validation checklist: what the petal enforces before signing
   - Key rules: idempotency, approval flow, permit expiry, signature handling

### Quality bar

Before shipping documentation, verify an agent can answer these questions
from the docs alone:

- Which tokens/chains are supported as origin?
- What does a valid write body look like for each operation type?
- What status will the transaction be in after a successful write?
- What should I do when the status is `approval_required`?
- What does error X mean and how do I fix it?
- What is the lifecycle from write to settlement?

If any answer requires reading source code, the docs are incomplete.

### `$index.rs` should list documentation files

The root `$index.rs` directory listing should explicitly include
`README.md` and `AGENTS.md` as served files, so directory traversal
discovers them:

```rust
petal::route_file!(
    spec: petal::static_dir_spec(),
    list: vec![
        petal::dir("transactions"),
        petal::file("status.json"),
        petal::file("README.md"),
        petal::file("AGENTS.md"),
    ]
);
```

>>>>>>> theirs
## Review checklist

Before handing off, confirm:

- every route's behavior is visible in its route file;
- shared code contains no filename- or path-driven endpoint dispatcher;
- every writable file defines safe local read behavior;
- route capabilities stay within compiled imports and cover successful paths;
- the package capability ceiling and per-surface policy cover only intended use;
- network egress is allowlisted to the narrowest host, method, and path set;
- no secret or signature material is reachable through a read handler;
- durable state does not overstate external completion;
- store key schemes are consistent between save and load for each state type;
- directory listings that return empty are called out as product gaps;
- if legacy/compat modules exist, the generic route handles all their cases;
- all route components and release packaging validate;
- no live write was performed without explicit authorization;
- `status.json` is self-documenting (description, token/chain reference,
  doc pointers);
- `README.md` and `AGENTS.md` cover all supported operations with examples;
- `AGENTS.md` has a state machine diagram and next-action/error tables;
- the root `$index.rs` lists documentation files as served entries;
- `petal.toml` has a `[source]` section (`kind = "github"`, `repository`).

### Automating the checklist

After manual review, run the automated guidelines check to catch mechanical
gaps. Copy `scripts/guidelines-check.py` from this skill into the petal repo
and run it from the repo root:

```sh
python3 scripts/guidelines-check.py
```

It checks 30+ items: petal.toml schema/source/consent, capability minimality,
no signing intents (unless intended), no path dispatch, no secret accessors in
route files, secret-namespace wiring, public-view field exclusions,
`is_safe_segment` usage, `deny_unknown_fields`, commitment/integrity checks,
$index.rs documentation listings, status.json self-documentation, README/AGENTS
quality bars, architecture script presence, SDK rev pin match, no vendored SDK,
no committed .wasm, and more. Add petal-specific checks to the script as needed.
