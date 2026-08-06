# VFS Smoke Testing: End-to-End Petal Validation

Concrete commands for testing a petal through Bloom's VFS after
packaging and release. Covers read-only routes, write-flow testing
without signing, and release-artifact validation.

## 1. Release-artifact validation via fresh `bloom init`

The strongest smoke test: provision from the actual GitHub release,
not a local build. This validates the release tag, archive, hash, and
`petal-release.json` manifest end-to-end.

```sh
export BLOOM_HOME=/tmp/bloom-smoke
export BLOOM_BIN=/root/dev/bloom-directory/bloom/target/release/bloom

rm -rf "$BLOOM_HOME"
mkdir -p "$BLOOM_HOME"
$BLOOM_BIN init
```

Verify the petal appears in the preinstalled list and installed cleanly:

```
preinstalled_petal: installing <name> from https://github.com/...@<sha>
preinstalled_petal: <name> ready at petals/<name>/
```

If this fails with a hash mismatch or 404, the release artifacts or
catalog pin are wrong — fix those before continuing.

## 2. Start daemon and verify VFS routes

```sh
# Start daemon in background (use bloom serve, not bloom vfs, for writes)
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN serve --mount /tmp/bloom-mount &

# Verify routes are served
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs ls /petals/<name>/ -q
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs cat /petals/<name>/status.json -q
```

Confirm:
- Expected route files are present (e.g. `transactions/`, `status.json`)
- No stale routes from deleted modules
- `status.json` returns valid JSON with correct provider/kind/operations

## 3. Write-flow testing with a watch-only wallet

**Key technique:** Create a watch-only wallet (cannot sign) to test the
transaction write flow end-to-end up to the signing gate. This exercises
quote validation, order safety checks, state persistence, and idempotent
state transitions — all without moving real funds or needing a signing
ceremony.

### Create a watch-only wallet via VFS

```sh
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs write \
  --data 'name = "smoke-test"
kind = "watch"
address = "0x03508bb71268bba25ecacc8f620e01866650532c"' \
  /wallets/new -q
```

Verify it resolved:

```sh
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs cat /wallets/smoke-test/address -q
# Should print the checksummed address
```

### Write a transaction (quote-only, no signing)

```sh
BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs write \
  --data '{
    "origin": {
      "chain": "ethereum",
      "chain_id": 1,
      "currency": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
      "decimals": 6,
      "permit_domain": {"name": "USD Coin", "version": "2"}
    },
    "destination": {
      "chain": "optimism",
      "chain_id": 10,
      "currency": "0x0b2c639c533813f4aa9d7837caf62653d097ff85",
      "decimals": 6,
      "recipient": "0x03508bb71268bba25ecacc8f620e01866650532c"
    },
    "amount": "100",
    "minimum_output": "90"
  }' \
  /petals/<name>/transactions/<wallet>/test-001.json -q
```

### What to verify

1. **Minimum-output validation works.** Set `minimum_output` above
   the upstream's actual quote output → the write should fail with
   an error indicating the minimum output floor was violated.
   This proves the petal is fetching live quotes and comparing them.

2. **Successful quote persists state.** With a valid `minimum_output`,
   the write returns `permission denied` (watch-only wallet can't sign),
   but the transaction state is persisted. Read it back:

   ```sh
   BLOOM_HOME=/tmp/bloom-smoke $BLOOM_BIN vfs cat \
     /petals/<name>/transactions/<wallet>/test-001.json -q
   ```

3. **State fields to confirm in the persisted JSON:**
   - Correct lifecycle status (e.g. `approval_required`)
   - Quote data from the upstream API (amounts, provider, timing)
   - Approval/signing ceremony is wired up (URL, expiry, retry body)
   - Submission fields are null — not yet submitted
   - Correct schema identifier

4. **Idempotent retry.** Write the same body again with the same
   `<wallet>/<id>` — it should resume from persisted state, not
   create a duplicate or re-quote.

## 4. Run the petal's own live-quote check script

Most petals that proxy an external API ship a validation script:

```sh
cd bloom-petal-<name>
bash scripts/live-quote-check.sh
```

This exercises the upstream API with assertions tailored to the petal's
trust contract (step structure, permit type, receiver pinning, payment/
refund/fee safety). All assertions should pass.

## 5. Multi-chain validation via direct API calls

Test all supported chains through the upstream API directly (not through
the petal) to confirm route availability. A single-chain test does not
prove multi-chain support — iterate over all chain IDs.

## Summary checklist

- [ ] `bloom init` provisions the petal from GitHub release
- [ ] VFS routes match expected tree (no stale routes from deleted modules)
- [ ] `status.json` returns correct capability info
- [ ] Watch-only wallet created and address resolves
- [ ] Transaction write with bad `minimum_output` is rejected
- [ ] Transaction write with valid `minimum_output` persists `approval_required`
- [ ] Persisted state contains live quote, request ID, ceremony URL
- [ ] Petal's own `live-quote-check.sh` passes all assertions
- [ ] Multi-chain API calls return valid responses for supported routes
