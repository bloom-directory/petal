---
name: bloom-petal-development
description: Build, refactor, review, and validate Bloom Petals that expose route-based virtual files through the Petal SDK and builder. Use when creating a Petal, adding or changing route files, designing route and shared-code boundaries, declaring capabilities, investigating oversized route components, or preparing a Petal for packaging and release.
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
`current_route_path`, filenames, path suffixes, or parameter presence to choose
behavior for many routes. Do not route through chains of `contains`,
`ends_with`, or string-keyed matches.

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

## Validate in layers

Use the repository's pinned commands and scripts. At minimum:

1. Format and run host-side unit tests.
2. Run clippy or the repository's equivalent static checks.
3. Build every route component, not only the shared host crate.
4. Run `petal check` against the generated package to compare declared
   capabilities with actual component imports.
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
- durable state does not overstate external completion;
- all route components and release packaging validate;
- no live write was performed without explicit authorization.
