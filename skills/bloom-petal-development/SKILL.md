---
name: bloom-petal-development
description: Build, refactor, review, and validate Bloom Petals that expose route-based virtual files through the Petal SDK and builder. Use when creating a Petal, adding or changing route files, designing route and shared-code boundaries, declaring capabilities, investigating oversized route components, or preparing a Petal for packaging and release.
license: MIT
metadata:
  author: bloom-directory
  version: 1.0.0
  category: development
  activation: intent
  tags:
    - petals
    - bloom
    - rust
    - wasm
    - routing
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
its generated structure instead of inventing a parallel layout.

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

Use the route macro's local `read`, `write`, or `read` plus `write` handlers.
A writable route that exposes instructions on read should define both handlers
in that route file.

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

Pass typed values from the route file into shared functions. Avoid passing a
route path or filename and asking shared code to infer the operation. A small
amount of repetition in route files is preferable to hidden coupling through
dynamic dispatch.

## Declare least capabilities

Match every route's metadata capabilities to the imports that remain in its
compiled component. Refactoring shared dispatch into local handlers often
removes imports through dead-code elimination, so revisit capability
declarations after architectural changes.

Use the narrowest applicable route spec, then override capabilities only when
the route genuinely imports a different set. Treat capability-check failures
as architecture feedback, not as errors to silence with broader declarations.

Network egress is part of least capability. Declare every reachable upstream
host in `petal.toml` under `[net.allow]` with explicit method and path
prefixes; never rely on a wildcard host. Shared HTTP helpers should resolve
targets from a fixed, enumerated source (for example a `Network` variant that
returns a `&'static str` URL) so route code cannot direct a request at an
arbitrary host. If a new upstream is introduced, widen `[net.allow]`
deliberately, not as a side effect of editing a helper.

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

Secret-key material, derived agent keys, and signatures are confidential.
Store them only in the store's secret namespace and never copy their bytes
into the public state namespace or into a read response. A read handler must
never project raw private keys, agent keys, or full signature blocks; when a
read needs to expose signing state, return only what an auditor needs (status,
intent, outcome, and non-secret identifiers).

When a route signs locally with a stored key, the route's declared capabilities
should omit the host signing capability and the signing function must live
behind a typed helper that cannot return key bytes to its caller. Treat the
boundary between the secret namespace and the readable VFS as a hard wall and
add a test that confirms no secret key is read by a public route.

## Validate in layers

Use the repository's pinned commands and scripts. At minimum:

1. Format and run host-side unit tests.
2. Run clippy or the repository's equivalent static checks.
3. Build every route component, not only the shared host crate.
4. Compare declared capabilities with actual component imports. The toolchain
   runs this comparison inside `petal build` (per component), again inside
   `petal package`, and on its own via `petal check` (without rebuilding);
   confirm the repository's CI invokes one of these rather than only
   compiling the host crate.
5. Run `petal package` or the repository's release-validation command.
6. Confirm the expected route count and inspect unexpected component-size
   changes.

Do not use a successful host-crate build as evidence that route sources compile;
route files are independently generated component crates.

## Review checklist

Before handing off, confirm:

- every route's behavior is visible in its route file;
- shared code contains no filename- or path-driven endpoint dispatcher;
- writable files define local read behavior where required;
- declared capabilities match actual imports;
- network egress is allowlisted to the narrowest host, method, and path set;
- no secret or signature material is reachable through a read handler;
- durable state does not overstate external completion;
- all route components and release packaging validate;
- no live write was performed without explicit authorization.
