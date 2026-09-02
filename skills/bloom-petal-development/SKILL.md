---
name: bloom-petal-development
description: Build, migrate, refactor, review, and validate Bloom Petals that expose domain behavior as route-based virtual files through the Petal SDK and builder. Use when turning an API or workflow into a Petal, creating a Petal, adding or changing route files, designing route and shared-code boundaries, declaring capabilities and manifest policy, investigating oversized route components, or preparing a Petal for packaging and release.
version: 0.1.0
---

# Bloom Petal Development

Build Petals as explicit virtual files whose behavior is understandable from
their route source. Keep endpoint policy local and share only substantial,
typed infrastructure.

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
or VFS paths. Before interpolating a value into a URL, enforce its strict
domain grammar or encode it as a URL path segment. Use `petal::is_safe_segment`
for VFS or storage path segments; it is not sufficient URL validation. Do not
assume supplied context parameters are safe merely because the source filename
uses brackets.

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
this as the primary enforcement. The toolchain's `petal check` rejects route
identity accessors in the shared route crate and reconciles every route's
declared capabilities against its compiled imports. Repository-level checks
may enforce additional local rules, but must not duplicate the SDK accessor
denylist.

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
- all route components and release packaging validate;
- no live write was performed without explicit authorization.
