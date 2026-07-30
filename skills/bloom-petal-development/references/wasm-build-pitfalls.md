# WASM Build Prerequisites and Component Compilation Pitfalls

Code that passes `cargo check` and `cargo test` in the host crate can still
fail during `petal build`, which compiles each route file as a standalone
WASM component crate. This reference covers the prerequisites and the specific
differences between host-crate compilation and component compilation.

## Prerequisites

`petal build` (via `scripts/build.sh`) requires three toolchain components
that are NOT installed by default on a fresh Rust toolchain:

```sh
# 1. WASM targets (both are needed — different route components use different targets)
rustup target add wasm32-wasip2
rustup target add wasm32-unknown-unknown

# 2. wasm-tools binary (version must compile on your rustc — latest usually works)
cargo install wasm-tools
```

The `build.sh` script installs the petal CLI itself from the pinned git rev
specified in `petal-build.toml`. This is automatic — you don't need to
install the CLI separately. The installed binary lands at
`target/petal-tool/bin/petal`.

If `build.sh` fails with `error: run command: No such file or directory`,
it means a required tool is missing — check `rustup target list --installed`
and `which wasm-tools`.

## Component compilation pitfalls

Each route `.rs` file becomes its own component crate. Code that works in
the shared host crate (where all modules are in scope) can fail in the
isolated component crate context.

### 1. Inner doc comments (`//!`) before macro invocation

**Fails:** `//! doc comment` immediately before `petal::route_file!` produces
`error[E0753]` and `unused_doc_comment` warnings in component compilation.

**Fix:** Use regular `//` comments instead of `//!` in route files:

```rust
// Wrong — fails in WASM component:
//! Blocking poll that reads balance every 5s.
petal::route_file!(...)

// Right:
// Blocking poll that reads balance every 5s.
petal::route_file!(...)
```

### 2. Missing trait imports for direct host method calls

**Fails:** A route file that calls `host.put(...)` or `host.get(...)`
directly gets `E0599: no method named X found` in component compilation,
even though the host crate compiles fine.

**Cause:** The `Host` trait is defined in `runtime.rs` and re-exported via
`workflow.rs`. In the host crate, it's always in scope. In a component crate,
it must be imported explicitly.

**Fix:** Add `use crate::workflow::Host;` inside the closure body:

```rust
write: |_ctx: &petal::Ctx, body: &[u8]| {
    use crate::workflow::Host;  // <-- required for host.put/get/etc
    let mut host = crate::workflow::BloomHost;
    host.put("key", value, true)
}
```

Routes that only call shared functions like `crate::workflow::create()` don't
need the import — the shared function handles trait scope internally.

### 3. Type inference failures with `serde_json::from_str`

**Fails:** `serde_json::from_str(r).ok()` produces `E0283: type annotations
needed` in component compilation. The host crate can infer the type from
context, but the component crate cannot.

**Fix:** Add explicit turbofish:

```rust
// Wrong — type inference fails in component crate:
.and_then(|r| serde_json::from_str(r).ok())

// Right:
.and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
```

### 4. Nonexistent spec functions

**Fails:** `petal::static_spec()` produces a compilation error — it does
not exist in the SDK.

**Fix:** Use `petal::static_read_spec()` for static read-only files.
Check the spec table in the skill's SKILL.md for the complete list.

### 5. Capability mismatch with compiled imports

**Fails:** `petal check` rejects routes with
`error: route X requires absent imports: bloom:chain` when a route declares
a capability in `.caps()` that the compiled WASM doesn't actually import.

**Also fails silently:** A route that DOES import a capability but doesn't
declare it in `.caps()` will build but `petal check` will reject it.

**Fix:** Ensure `.caps()` exactly matches the capabilities the route's
compiled code actually imports. Use `wasm-tools component wit <file>.wasm`
to inspect actual imports, then compare against the spec declaration.

Common mismatch patterns:
- Route declares `bloom:chain` but only does store reads → remove the cap
- Route uses store reads but only declares `bloom:chain` → add `bloom:store`
- Route overrides `.caps()` and accidentally drops a required cap

### 6. `PETAL_BIN` environment variable

The `build.sh` script may expect `PETAL_BIN` to point to the installed CLI.
If running `bash scripts/build.sh` directly after a failed first run, set:

```sh
PETAL_BIN=./target/petal-tool/bin/petal bash scripts/build.sh
```

### 7. `bloom petals install` fails after source modification

After modifying route source code and running `petal build`, `bloom petals
install` may fail with:

```
error: invalid wasm: Petal package artifact r000006 does not match its route source
```

**Cause:** Bloom's `package.rs` validates that each generated WASM artifact
hash-matches its route source. Three generated layers may be stale after a
source change:

- `petal/<name>` — Petal CLI route package;
- `target/petal-routes` — generated component workspace and compilation cache;
- `artifacts` — Bloom-composed `rXXXXXX.wasm` artifacts.

The last layer is easy to miss. Bloom validates an existing
`artifacts/routes/rXXXXXX.wasm` before it gets a chance to regenerate it, so
cleaning only the Petal CLI outputs can reproduce the same failure
deterministically.

**Fix:** Full clean rebuild — remove or move all three generated layers:

```sh
rm -rf -- petal/<name> target/petal-routes artifacts
PETAL_BIN=./target/petal-tool/bin/petal ./scripts/build.sh
```

Then have Bloom regenerate and validate its composed artifacts before install:

```sh
cd /root/dev/bloom-directory/bloom
./target/debug/bloom petals build /path/to/bloom-petal-<name>
./target/debug/bloom petals install /path/to/bloom-petal-<name>
```

If replacement of an installed petal is needed, use the supported uninstall
command and reinstall:

```sh
./target/debug/bloom petals ls
./target/debug/bloom petals uninstall <name-or-hash>
./target/debug/bloom petals install /path/to/bloom-petal-<name>
```

## Verification sequence after fixes

```sh
# 1. Host crate still compiles
cargo check

# 2. Host tests pass
cargo test

# 3. Architecture check
bash scripts/check-route-architecture.sh

# 4. Full WASM build (incremental — only rebuilds changed components)
PETAL_BIN=./target/petal-tool/bin/petal bash scripts/build.sh

# 5. Capability validation
./target/petal-tool/bin/petal check --root .
```
