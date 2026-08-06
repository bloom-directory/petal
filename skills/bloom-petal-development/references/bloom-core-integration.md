# Integrating a Petal into Bloom Core

After a petal is built, tested, and packaged, register it as a default
preinstalled petal. There are **two distinct integration tracks**
depending on how the petal is loaded:

- **Dynamically-loaded petals** (the common case) — installed as WASM
  packages from GitHub releases.
  Their petal_id is `petal:<name>` (via `PETAL_PETAL_ID_PREFIX`), which
  is handled generically by `is_petal_petal_id()`. They do NOT need
  `PETAL_ID_*` constants or `placeholder_digest_for()` entries.
- **First-party built-in petals** — compiled into bloom core with static
  identity constants. These are rare and handled case-by-case.

This guide covers the dynamically-loaded (WASM package) track.

## 1. Catalog entry — `bloom/crates/bloom/src/github_source.rs`

**This is the most important integration point.** Without it, `bloom init`
cannot download the petal.

Add a commit constant and a `PreinstalledPetal` struct alongside the
existing entries:

```rust
const PETAL_NAME_RELEASE_COMMIT: &str = "<40-char SHA>";

const PREINSTALLED_PETAL_NAME: PreinstalledPetal = PreinstalledPetal {
    name: "<name>",
    repository: "https://github.com/bloom-directory/bloom-petal-<name>",
    commit: PETAL_NAME_RELEASE_COMMIT,
    release_tag: "v<version>",
    archive: "<name>-v<version>.petal.tar.gz",
    expected_hash: Some("<blake3 package_hash>"),
};
```

Then add a match arm in `preinstalled_petal()`:

```rust
fn preinstalled_petal(name: &str) -> Option<&'static PreinstalledPetal> {
    match name {
        "<name>" => Some(&PREINSTALLED_PETAL_NAME),
        "<other-name>" => Some(&PREINSTALLED_NEAR_INTENTS),
        "<name>" => Some(&PREINSTALLED_PETAL_NAME),
        _ => None,
    }
}
```

**Getting the package hash:** Build and package the petal, then read
the `package_hash` from the output:

```sh
./target/petal-tool/bin/petal package --root . --out /tmp/<name>.petal.tar.gz
# Look for "package_hash" in the JSON output
```

## 2. Default list — `bloom/crates/bloom-proto/src/config.rs`

Add the petal name to `default_preinstalled_petals()`:

```rust
fn default_preinstalled_petals() -> Vec<String> {
    vec![
        // "<name>".to_string(),
        // "<other-name>".to_string(),
        // "<name>".to_string(),
    ]
}
```

Also update the **validation** in `validate()` that checks known names:

```rust
if !matches!(name.as_str(), "<name>" | "<other-name>") {
    return Err(ConfigError::Invalid(format!(
        "unknown preinstalled Petal {name:?}"
    )));
}
```

## 3. Update test assertions

Several tests assert the exact default preinstalled list. Search for them:

```sh
grep -rn '<name>.*<other-name>\|preinstalled.*==' crates/ --include="*.rs"
```

Update every assertion that checks the default list, the legacy
<name> opt-out migration, and the validation match. Typical locations:

- `config.rs` — `local_default_validates`, legacy <name> opt-out test
- `github_source.rs` — `preinstalled_petal()` test cases

## Updating pins after significant petal changes

When a registered petal undergoes structural changes (routes added or
removed, modules deleted, signing intents changed), the package hash
changes and bloom core's catalog pin becomes stale. `bloom init` will
reject the petal with a hash mismatch. The full repin workflow:

1. **Commit and push** the petal changes to the petal repo's branch.
2. **Rebuild and repackage** to get the new `package_hash`:
   ```sh
   cd bloom-petal-<name>
   bash scripts/build.sh
   ./target/petal-tool/bin/petal package --out /tmp/<name>-new.petal.tar.gz
   # Read package_hash from JSON output
   ```
3. **Verify determinism** — rebuild again and compare the hash. If it
   matches, the hash is stable.
4. **Update `github_source.rs`** — both the commit constant and
   `expected_hash` field in the `PreinstalledPetal` struct. The commit
   constant must use the **full 40-character SHA** (matching the pattern
   of all other petals — never a short 7-char prefix). Also update
   `release_tag` and `archive` if the version bumped.
5. **Update `AGENTS.md`** — deleted route files leave dangling references.
   Check every route path in AGENTS.md against the actual `route/files/`
   tree and remove references to deleted routes.
6. **Check `petal.toml`** — removed signing intents must be removed from
   `[sign].allowed_intents`. Removed capabilities must be removed from
   `[caps].allowed`.
7. **Run bloom core tests** with `--test-threads=1` (parallel builds
   race on shared build scripts):
   ```sh
   cd bloom
   cargo test -p bloom -- --test-threads=1
   cargo test -p bloom-proto config::tests
   ```
8. **Commit and push** the bloom core feature branch.


## Signing and identity (NOT needed for dynamically-loaded petals)

Dynamically-loaded petals get `petal_id = "petal:<name>"` at runtime via
`PETAL_PETAL_ID_PREFIX` in `bloom-daemon/src/lib.rs` (`petal_execution_origin`).
The `is_petal_petal_id()` function in `bloom-auth-api/src/lib.rs` recognizes
this format generically — no per-petal registration is needed.

Signing intents declared in `petal.toml` `[sign].allowed_intents` are
enforced at runtime through the grant's `allowed_sign_intents` list in
`DaemonPetalHost::petal_action_with_identity()`, not through static
registry entries.

Only first-party built-in petals need `PETAL_ID_*` constants, `placeholder_digest_for()`
entries, and `DefaultAttestationRegistry::allowed_pair()` match arms in
`bloom-auth-api/src/lib.rs`.

### Verifying signing works without a live ceremony

When integrating a new petal that uses `bloom:sign`, you may need to
confirm the signing path works without performing a live signing
ceremony (which requires wallet interaction). Trace these **four gates**
in order — all are generic for `petal:*` petals:

1. **Schema registration** — the petal's signing intents are declared in
   `petal.toml` `[sign].allowed_intents`. Bloom reads these at install
   time. No core code change needed.
2. **Attestation validation** — `PetalSigningAttestationFacts::validate()`
   (`bloom-auth-api/src/lib.rs` ~line 1609) checks only **structural**
   fields (non-empty intent, valid format). It has **zero intent-specific
   logic**. The only intent-specific validation in the auth-api is in
   `validate_attestation()`, which special-cases `PETAL_ID_PAID_HTTP`
   only. All other `petal:*` ids pass generically.
3. **Grant enforcement** — `DaemonPetalHost::petal_action_with_identity()`
   checks the grant's `allowed_sign_intents` list. If the intent string
   matches a `petal.toml`-declared intent, the sign is permitted.
4. **VM intent gating** — `bloom-petals/src/vm.rs` `sign_intent_allowed()`
   checks the WASM component's declared intent against the grant. Again
   generic — no per-petal hardcoding.

If all four gates use generic `petal:*` matching (which they do by
design), no auth-api changes are needed for a new dynamically-loaded
petal.

## Package hash determinism

Petal package hashes are deterministic: rebuilding from the same source
with the same toolchain produces the same `expected_hash`. You do not need to update the catalog entry after
a clean rebuild — if the hash changes, something in the source or
toolchain actually changed.

## Commit pinning before merge to main

When registering a petal whose audit-fix branch hasn't been merged to
`main` yet, you can safely pin the catalog's `commit` field to the
branch HEAD. To confirm this is safe:

```sh
# Verify main is a direct ancestor of the branch
git merge-base --is-ancestor main <branch> && echo "fast-forward possible"
```

If main is an ancestor, a fast-forward merge later will not create a
new commit SHA — the pinned SHA remains the canonical commit on `main`.
This lets you wire bloom core before the petal PR is merged.

## GitHub release (required for preinstalled download)

`bloom init` downloads preinstalled petals from GitHub releases. The
release must contain:

- `petal-release.json` — manifest with package hash and artifact list
- `<archive>.petal.tar.gz` — the packaged petal
- `SHA256SUMS` — checksum file

Without a published release, `bloom init` will 404 with:

```
error: provision configured pre-installed Petals: provision pre-installed
Petal <name> from <repo> release <tag> at commit <sha>; fix the cause and
retry `bloom init`, or persistently opt out with `[petals] preinstalled = []`
```

## Smoke testing locally

There are two smoke test approaches, each validating different things:

- **Release-artifact test** (recommended after publishing): run `bloom init`
  with a fresh `BLOOM_HOME` and let it download the petal from the GitHub
  release. This validates the release tag, archive, hash, and manifest
  end-to-end. No local install workaround needed.

- **Local build test** (during development, before release): install a
  local build directly. Requires the provenance workaround below.

See `references/vfs-smoke-testing.md` for the full end-to-end smoke test
reference, including **write-flow testing with a watch-only wallet** —
the technique for exercising quote validation, order safety checks, and
state persistence without needing a signing wallet or moving funds.

### Release-artifact smoke test (post-publish)

```sh
export BLOOM_HOME=/tmp/bloom-smoke
export BLOOM_BIN=/root/dev/bloom-directory/bloom/target/release/bloom

rm -rf "$BLOOM_HOME"
mkdir -p "$BLOOM_HOME"
$BLOOM_BIN init                   # downloads petal from GitHub release
$BLOOM_BIN serve --mount /tmp/bloom-mount &
$BLOOM_BIN vfs ls /petals/<name>/ -q
$BLOOM_BIN vfs cat /petals/<name>/status.json -q
```

### Local build smoke test (pre-publish)

```sh
# 1. Build WASM components
cd bloom-petal-<name>
bash scripts/build.sh

# 2. Package and get the hash
./target/petal-tool/bin/petal package --root . --out /tmp/<name>.petal.tar.gz

# 3. Install into a test bloom home
export BLOOM_HOME=/tmp/bloom-smoke
rm -rf "$BLOOM_HOME"
bloom init                    # provisions other preinstalled petals
bloom petals install .        # installs local petal package

# 4. Workaround: remove the petal from preinstalled list in config
#    (bloom serve rejects locally-installed petals in the preinstalled
#    list because they lack GitHub source provenance)
#    Edit $BLOOM_HOME/config.toml [petals] preinstalled = [...]

# 5. Start daemon
bloom serve &

# 6. Test routes via VFS IPC
bloom vfs ls /petals/<name>/
bloom vfs cat /petals/<name>/status.json
bloom vfs ls /petals/<name>/chains/
```

### Pitfall: provenance check on `bloom serve`

`bloom serve` also runs `ensure_preinstalled_petals()`. If a petal is in
the preinstalled list but was installed locally (not from GitHub), the
daemon refuses to start:

```
error: pre-installed Petal <name> is already owned by an installation
without source provenance; uninstall it or set `[petals] preinstalled = []`
```

**Fix:** Remove the petal from `preinstalled` in `config.toml` before
starting the daemon. The petal stays installed and its routes remain
available.

### Testing external API connectivity (quote-only)

For petals that proxy an external API, verify the upstream
directly with curl before testing through the petal:

```sh
curl -s -X POST https://api.<upstream>/quote \
  -H 'Content-Type: application/json' \
  -d '{...}' | python3 -m json.tool
```

Quote-only flows are safe — they contact external APIs but do not move
funds or require signing.

**Test every supported chain.** A petal may support multiple chains
(Ethereum=1, Base=8453, Arbitrum=42161, Optimism=10, Polygon=137,
Avalanche=43114). Iterate over all chain IDs with a curl call each —
a single-chain test does not prove multi-chain support. All chains
should return a valid quote response with the expected `op` field.

## Key files

| File | Role |
| --- | --- |
| `bloom/crates/bloom/src/github_source.rs` | `PreinstalledPetal` catalog, `preinstalled_petal()` match, release download |
| `bloom/crates/bloom-proto/src/config.rs` | `default_preinstalled_petals()`, validation, test assertions |
| `bloom/crates/bloom-auth-api/src/lib.rs` | `is_petal_petal_id()`, `PETAL_ID_*` (first-party only), `DefaultAttestationRegistry` |
| `bloom/crates/bloom-daemon/src/lib.rs` | `DaemonPetalHost`, `petal_execution_origin()`, signing flow |
| `bloom/crates/bloom-keystore/src/petal_host.rs` | `KeystorePetalHost`, grant enforcement, `allowed_sign_intents` check |
| `bloom/crates/bloom-petals/src/vm.rs` | WASM runtime, `sign_intent_allowed()`, capability enforcement |
| `bloom-petal-<name>/petal.toml` | Package manifest: name, source, caps, net/sign/store policy |
