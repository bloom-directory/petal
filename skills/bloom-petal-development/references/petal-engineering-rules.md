# Petal Engineering Rules

House rules for petal code. Each one exists because an agent shipped the
opposite: a route that grew a second dispatcher, a signing digest that moved
without a failing test, a `#[allow]` over a real bug, a comment that outlived
the line it described.

Apply them while writing and again while auditing. Where a target repository's
own `AGENTS.md` states a different rule, that repository wins.

## Route files

- A route file is **exactly one** `petal::route_file!(...)` invocation. Pick the
  narrowest spec (see SKILL.md § Select the local handler and spec) and narrow
  further with `.caps(&[...])` to what the handler actually uses — the manifest
  intersects them.
- **Never hand-write `__PetalRouteIdentity`.** The builder injects it. A
  hand-written identity desynchronizes the component from its generated route.
- A read handler returns `DispatchResponse::Read` or `Error`; a write handler
  returns `DispatchResponse::Write` or `Error`.
- **A write carries no body out.** `DispatchResponse::Write` is a payload-free
  variant — the result of a write is read back from a sibling `.json` route.
  That is the VFS idiom; do not fight it.
- Keep route files thin: extract parameters with `petal::param`, then call one
  `crate::<module>::<fn>`. All real logic lives in `route/src/`.
- A writable route's `read` branch is its own documentation — it returns the
  one-line usage string for its body. Keep it accurate or delete it.

## Signing and keys

- **Keys never enter the petal.** Hash in a dedicated module (`eip712.rs` or
  equivalent) and sign through a typed helper such as
  `signing::sign_hash(wallet, owner, hash, intent)`. No key bytes, no `Signer`,
  no private key in petal code, ever.
- Every intent must appear in `petal.toml` `[sign].allowed_intents`. The
  recovered signer is verified against the session owner inside the signing
  helper — do not bypass that by hashing and dispatching somewhere else.
- **Never widen a signing digest without a pinned test.** Pin the domain
  separator, one known-good vector per signed struct, and any hand-packed
  `meta` blob. A domain or struct refactor that shifts a hash must break a test.
- A value-moving intent comes back as `ApprovalRequired`, not a signature.
  Persist the ceremony artifact and return the retry-shaped error. Do not
  invent a fallback that signs anyway.

## Types and structure

- Don't mirror a config or wire struct into a second runtime struct. Use it
  directly.
- Configure through `petal.toml` and the daemon's runtime settings — the
  `bloom:env` `setting` function, reached as `petal::sdk::runtime_setting`. A
  behavioural tunable is not a bare `const`, and never a CLI flag.
- A fixed set of choices is an enum, not strings compared with `==`. Use the
  typesystem.
- Use serde derives. Hand-write a (de)serializer only when the wire genuinely
  demands it — and then test that one.
- When you move or replace code, fix the whole import path. A re-export shim at
  the old path is dependency spaghetti and is lazy.
- Serialize `U256` as a decimal string on the wire; accept decimal or `0x`-hex
  on the way in.
- **Compute on real Ethereum types** — alloy `Address`, `B256`, `U256`. Parse
  wire strings into them before any hashing, comparison, or arithmetic. Never
  do arithmetic or address comparison on raw strings or `Vec<u8>`, and never
  hand-pad hex to build calldata when `sol!` + `abi_encode` will do it.
  Case-insensitive string comparison is not the fix for a checksum-vs-lowercase
  address bug; parsing is. `defi-chain-patterns.md` works the boundary end to
  end — typed `Host` trait, enum chain and status, decimal strings rendered
  only at the SDK call.

## Behaviour

- **Don't make things up.** If you didn't read it, it isn't real. A claim needs
  evidence: a `file:line`, a command's output, a response body.
- Don't hardcode. Use `petal.toml` plus runtime settings.
- `HashMap` and `HashSet` are unordered. Where iteration order reaches output —
  a directory listing, a hash preimage, a digest — sort explicitly or keep a
  list.
- Build in release. `petal build` compiles routes `--release`; never judge
  behaviour or component size from a debug build.
- **Fading a clippy or rustc warning is banned.** A warning is the compiler
  telling you the code is wrong. Fix it; don't `#[allow(...)]` it away. Lint the
  shared crate, where the logic lives — a generated route crate is glue.
- When you update a part, identify everything it makes obsolete and delete it.
  No dead code, no "just in case" leftovers.
- The HTTP wire contract a petal speaks is defined by the server, not by the
  petal repo. When a DTO, endpoint, or signing detail changes upstream, mirror
  it — don't guess, and don't "fix" a mismatch by loosening a type to
  `serde_json::Value` or `Option`.

## Tests

- A test that asserts an `if`, a `>=`, or basic arithmetic works is useless.
  Language primitives work; wrapping them in a function doesn't change that.
- Don't test that serde round-trips. It does. Only test a (de)serializer you
  wrote.
- A good test enforces new, non-trivial logic this code added, or a crucial
  invariant. Pinned EIP-712 digests and hand-packed byte layouts earn their
  keep. So does a lifecycle test through a mock `Host`
  (`petal-integration-testing.md`).

## Comments

- **No explanatory comments in the code body.** Express the property in the
  code. An essay over a line doesn't change its behaviour; it rots and misleads
  the next agent.
- A one-line `//!` module header and a one-line `///` on a public item is the
  maximum. No multi-paragraph doc essays. In route files use `//` — an inner
  doc comment before `route_file!` fails component compilation
  (`wasm-build-pitfalls.md` § 1).
- Struct fields get at most one short sentence each.
- A test may carry a one-sentence purpose comment, nothing more.
- **Errors and logs are not comments.** A failure is
  `DispatchResponse::Error { code, message }` with a terse label — `-1` not
  found, `-2` denied, `-3` invalid, `-4` backend — never a `format!` prose
  essay explaining what went wrong.
- No comments in `.toml` or `.sh`.

## Functions

- Only extract a function that is actually reused, or whose logic is complex
  enough to warrant a test (signing-hash helpers qualify — they are pinned). A
  once-called wrapper over two lines is boilerplate; inline it.
- Don't write functions with no logic. A function that takes four values and
  adds them is boilerplate.
- Don't write speculative helpers. If nothing calls it, don't write it.
- **Too many arguments is a smell, not a thing to
  `#[allow(clippy::too_many_arguments)]`.** Group them into a struct. If the
  arguments are the fields of a struct you already have — or of the value you
  return — take that struct in. A builder is not the answer.

## Variables

- Don't manufacture variables just to pass them on. If they are the fields of a
  struct, pass the struct or put the function on it. Twenty lines of shuffling
  values into a call is not code. A guard-unwrap local — unwrapping a `Result`
  before continuing — is fine.

## Errors

- Map a host or HTTP error to `DispatchResponse` **once at the boundary**
  (`sdk_error`, the HTTP-status mapping). Don't hand-translate the same failure
  at every call site.
- Chain library errors into your own enum with `thiserror` `#[from]` so `?`
  works. Never `map_err(|e| MyError::X(e.to_string()))` — that discards the
  type for a string. A host error keeps its `HostStatus`; it is never flattened
  into a `String` variant.
- Never format an error into a string and use the string as the error type.
- Never return an `Option` as an error signal. Return a `Result` or
  `DispatchResponse::Error`. `None` is only for a genuine, correct absence.
- No `.unwrap()` / `.expect()` on a host, network, or parse result. Tests are
  the exception.

## Build and validate

- Run the repository's own scripts, which wrap the vendored CLI at
  `./target/petal-tool/bin/petal` (`PETAL_BIN` reuses an already-built CLI
  instead of `cargo install`). Without them it is `petal build --root .` then
  `petal check --root .`, ending at the `bloom petals build` package gate when
  `bloom` is available.
- Generated wasm and `target/` are git-ignored. Do not commit
  `petal/<name>/**/*.wasm` or `artifacts/`.
- Adding a capability, host, path, or intent means editing `petal.toml`. **An
  installed petal whose capabilities changed must be uninstalled first** — the
  daemon rejects the re-install with `cap mismatch`. That is a different
  failure from the stale-artifact `does not match its route source`
  (`wasm-build-pitfalls.md` § 7); a clean rebuild will not clear it.

## Git

Don't push, commit, stage, or create branches or worktrees. Write on the
current branch and leave the commit to the user.
