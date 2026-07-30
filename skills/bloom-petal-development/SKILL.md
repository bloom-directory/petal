---
name: bloom-petal-development
description: Build, migrate, refactor, review, and validate Bloom Petals that expose domain behavior as route-based virtual files through the Petal SDK and builder. Use when turning an API or workflow into a Petal, creating a Petal, adding or changing route files, designing route and shared-code boundaries, declaring capabilities and manifest policy, investigating oversized route components, auditing Petal code against the engineering rules, or preparing a Petal for packaging and release.
---

# Bloom Petal Development

Build Petals as explicit virtual files whose behavior is understandable from
their route source. Keep endpoint policy local and share only substantial,
typed infrastructure.

## Load the engineering rules

`references/petal-engineering-rules.md` is the line-level rule set for Petal
code: route-file shape, key and signing discipline, types, errors, comments,
tests, and the build gate. Read it before writing route or shared-crate code,
and again when auditing code someone else wrote — it is the standard a review
is measured against. Where a target repository's own `AGENTS.md` states a
different rule, that repository wins.

## Orchestrate implementation and audit

Treat non-trivial Petal work as orchestration rather than typing. Delegate
implementation to a subagent, and the audit of that implementation to a
different one. Each starts with fresh context and its own budget, which is what
keeps a migration or a gap-analysis pass from stopping halfway with a summary
of what remains. Tell each subagent that it is a child so it does not spawn its
own children, and give it the specific route files, references, and rules it
needs rather than the whole repository.

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
reusable behavior rather than endpoint selection. For concrete code patterns
observed across existing petals — sync HTTP adapters, `BloomHost` trait
implementation, credential resolution, session lifecycle, store-backed
directory listing, manifest shapes, and PETAL_REV pinning — see
`references/petal-scaffolding-patterns.md`. For DeFi/blockchain petals —
Host trait with chain helpers, ERC-20 allowance flow, eth_call simulation,
settlement verification via balance deltas, and multi-intent sessions — see
`references/defi-chain-patterns.md`. For WASM build prerequisites and
component compilation pitfalls (missing targets, trait import scoping, type
inference, doc comments, capability mismatches), see
`references/wasm-build-pitfalls.md`. For integration testing patterns (mock
Host trait, in-memory store, testing full workflow lifecycles without the SDK),
see `references/petal-integration-testing.md`. Good shared candidates
include:

- protocol types, serialization, hashing, and signing;
- bounded HTTP, store, chain, transaction, and VFS adapters;
- idempotency and durable state machinery;
- domain validation or policy used identically by multiple routes;
- multi-step operations with typed inputs and outputs.

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
2. Run integration tests that exercise the full workflow lifecycle through a
   mock `Host` implementation (see `references/petal-integration-testing.md`).
   These catch bugs that unit tests miss — state machine transitions, session
   serialization roundtrips, address format mismatches, multi-intent ordering.
3. Run clippy or the repository's equivalent static checks.
4. Run the repository's route-architecture source check.
5. Ensure WASM build prerequisites are installed: `rustup target add
   wasm32-wasip2 wasm32-unknown-unknown` and `cargo install wasm-tools`.
   These are not installed by default and cause cryptic build failures.
6. Run `petal build --root <root>` to compile every route component.
7. Run `petal check --root <root>` against the just-built components.
8. Inspect changed components' imports and compare all three policy scopes.
9. Run `petal package --root <root> --out <versioned>.petal.tar.gz` or the
   repository's release-validation command. Packaging refuses to overwrite an
   existing archive.
10. Run the pinned Bloom package-validation command, normally
   `bloom petals build <root>`, and exercise the target repository's VFS smoke
   tests when available.
11. Confirm the expected route count and inspect unexpected component-size
   changes.
12. **Install and drive the petal in a live bloom runtime.** After all prior
    layers pass, install the petal (`bloom petals install <path>`; uninstall
    first when its capabilities changed, or the daemon rejects the install
    with `cap mismatch`) and test every VFS route against real upstream APIs
    via `bloom vfs write/cat/ls`.
    This catches wiring issues that mock tests cannot — capability routing,
    store/VFS host binding, gas estimation, real API response shapes, and
    outbox staging. See `references/live-runtime-testing.md` for the full
    workflow: install → inspect → create intent → confirm/abandon →
    settlement → security guards → input validation → cross-chain.

Do not use a successful host-crate build as evidence that route sources compile;
route files are independently generated component crates. Code that passes
`cargo check` can still fail in WASM component compilation due to trait import
scoping, type inference differences, inner doc comments before macros, and
nonexistent spec functions. See `references/wasm-build-pitfalls.md` for the
prerequisites, specific errors, and fixes. `petal check` does
not rebuild, so do not use it against stale artifacts. Packaging checks built
routes but does not fully validate `petal.toml` policy and does not replace
route-level tests or a runtime VFS smoke test.

## Migrate native bloom functionality to a standalone petal

When a feature lives natively in the bloom monorepo (as a domain crate +
VFS handler) and needs to be extracted into its own petal repo, follow this
sequence. See `references/bloom-architecture.md` for the monorepo layout,
integration-point checklist, WIT capability map, and petal repo ecosystem.

1. **Inventory the native code.** Identify the domain crate (e.g.
   `crates/bloom-defi/`) and the VFS handler (e.g.
   `crates/bloom-vfs/src/handlers/defi.rs`). Map every VFS path the handler
   serves — these become route files.
2. **Name the petal after the external service** (`bloom-petal-enso`, not
   `bloom-petal-defi`). The VFS mount path can differ from the petal name.
3. **Map integration points to petal capabilities.** The VFS handler's
   dependencies translate directly: HTTP calls → `bloom:http`, in-memory
   state → `bloom:store`, `TxEngine::stage` → `bloom:tx.outbox`, chain reads
   → `bloom:chain`, wallet/address-book reads → `bloom:vfs.read`.
4. **Scaffold from `petal new`** and study the most complete existing petal
   (currently `bloom-petal-polymarket`) for the route-file pattern, shared
   crate organization, and manifest policy.
5. **Port domain logic** into `route/src/` (protocol types, HTTP adapters,
   signing helpers). Port VFS path handlers into individual `route/files/`
   route components — one `.rs` file per virtual path. See
   `references/petal-scaffolding-patterns.md` for the concrete code shapes
   (sync HTTP migration, `BloomHost`, credential resolution, session
   lifecycle, manifest templates). For DeFi/blockchain petals, see
   `references/defi-chain-patterns.md` (Host trait chain helpers, ERC-20
   allowance flow, eth_call simulation, settlement via balance deltas,
   multi-intent sessions).
6. **Run a gap analysis.** After the initial port compiles and tests pass,
   do a systematic line-by-line comparison of every VFS handler method in
   the original code against the petal's route files and shared crate. Then
   compare the route tree against sibling petals (polymarket, near) for
   ceremony lifecycle completeness — petals that stage transactions should
   expose `review_intent.json`, `outbox.json`, `receipt.json`, `abandon`, and
   `latest` in addition to the original create/confirm/wait/settle paths.
   See `references/defi-chain-patterns.md` § Ceremony lifecycle routes.
   Categorize gaps as critical / important / nice-to-have. Write the results
   to `GAP_ANALYSIS.md` in the petal root. This step has consistently surfaced
   30+ critical gaps that compile and test fine but represent missing
   behavior (sequential tx dependencies, confirm-time re-verification,
   missing route files, incomplete plan output, missing ceremony endpoints).
   See `references/migration-gap-analysis.md` for the methodology and
   checklist.

   6a. **Run a code quality audit.** After the gap analysis is closed and all
   tests pass, do a systematic pass for dead code, duplication, and logic
   errors that compile clean but waste lines or hide bugs. Common findings:
   unused API client functions (only one endpoint actually used), unused Host
   trait methods, dead struct fields, duplicated policy/simulation logic
   across route files, redundant computations made dead by earlier guards,
   intermediate saves before error returns. See
   `references/post-migration-code-audit.md` for the methodology and
   checklist.
7. **Wire `petal.toml`** with the capability ceiling, `[[net.allow]]`
   entries for every upstream host, signing intents, and store namespaces.
8. **Close gaps in priority order.** Work through the gap analysis:
   rewrite the domain layer in phases (foundation types → core workflow →
   route files), then re-run the comparison. Update the gap analysis as
   items close.
9. **Remove from bloom**: delete the domain crate, remove the VFS handler,
   disconnect workspace deps, daemon wiring, and config. Update docs and
   Docker test profiles. See the integration-point checklist in
   `references/bloom-architecture.md`.
10. **Add to bloom's preinstalled petals** in `bloom-proto/src/config.rs`
   `default_preinstalled_petals()` if the petal should ship by default.

## Pitfall: patch tool lint false positives on Rust async code

The Hermes `patch` tool runs a built-in syntax lint using **Rust 2015
edition**, which produces massive false-positive error lists for any file
containing `async fn`, `let chains` (`if let ... && let ...`), or
`async move` blocks. These errors say things like:

```
error[E0670]: `async fn` is not permitted in Rust 2015
  --> file.rs:82:5
   |
82 |     async fn resolve_name(...) -> ...
   |     ^^^^^ to use `async fn`, switch to Rust 2018 or later
```

This is noise — the actual bloom workspace uses edition 2024 and compiles
fine. **`cargo check` is the source of truth.** After any `patch` on a Rust
file with async code, verify with `cargo check` rather than reading the
lint output. The lint output's own summary line says
`"Pre-existing lint errors — this edit didn't introduce new ones"` when the
errors are not new.

This affects all bloom crates (`bloom-tx`, `bloom-evm`, `bloom-daemon`, etc.)
since they all use async tokio code.

## Pitfall: petal CLI is vendored per-repo, not system-installed

The `petal` binary is not on `$PATH`. Each petal repo vendors it at
`./target/petal-tool/bin/petal`. The validation steps above that say
`petal build` / `petal check` should use either:

- the repo's own scripts (`scripts/build.sh` invokes the correct binary
  automatically), or
- the explicit vendored path: `./target/petal-tool/bin/petal check --root .`

Running `petal` bare will produce `command not found`.

## Pitfall: check-route-architecture.sh route count

The `scripts/check-route-architecture.sh` script hardcodes the expected
route count. When adding or removing route files, update the count in the
script or the architecture check will fail. Count all `.rs` files under
`route/files/` recursively to get the correct number.

## Pitfall: petal SDK strips host error messages

The petal SDK's `host_err()` classifies errors by substring match and returns
generic `HostStatus` variants (`NotFound`, `Denied`, `Invalid`) instead of the
original message. Any host error containing the word "invalid" becomes the
opaque string `"invalid"` with zero diagnostic context. This is the #1 cause
of mysterious `"error": "invalid"` in simulation results — the route may be
valid but a parameter formatting issue is hidden by the error pipeline.

To debug, patch `chain_read` in `runtime.rs` to preserve `SdkError::Message`
text and add params context, and/or verify the `eth_call` independently via
`curl` against the chain RPC. See
`references/live-runtime-testing.md` § Debugging: SDK error message stripping
for the full technique.

## Pitfall: `bloom petals install` fails after partial WASM rebuild

After modifying route source and running `petal build`, `bloom petals install`
can fail with `"Petal package artifact rXXXXXX does not match its route
source"`. Treat all three generated layers as stale: the route package,
Petal CLI component workspace, and Bloom-composed artifacts. Remove or
preserve `petal/<name>`, `target/petal-routes`, and `artifacts`, then rebuild.
Cleaning only the route package is insufficient because Bloom validates an
existing `artifacts/routes/rXXXXXX.wasm` before regenerating it. See
`references/wasm-build-pitfalls.md` § 7 for details.

## Review checklist

Before handing off, confirm:

- every route's behavior is visible in its route file;
- each route file is one `route_file!` invocation over a thin parameter
  extraction, with no hand-written `__PetalRouteIdentity`;
- shared code contains no filename- or path-driven endpoint dispatcher;
- every writable file defines safe local read behavior;
- route capabilities stay within compiled imports and cover successful paths;
- the package capability ceiling and per-surface policy cover only intended use;
- network egress is allowlisted to the narrowest host, method, and path set;
- no key bytes, signer, or secret reach Petal code, and every intent is listed
  in `[sign].allowed_intents`;
- signing digests and hand-packed byte layouts are pinned by tests;
- no secret or signature material is reachable through a read handler;
- durable state does not overstate external completion;
- clippy and rustc are clean without an added `#[allow(...)]`;
- everything the change made obsolete is deleted;
- no explanatory body comments, prose error strings, or `.unwrap()` on host,
  network, or parse results survive (`references/petal-engineering-rules.md`);
- all route components and release packaging validate;
- no live write was performed without explicit authorization.
