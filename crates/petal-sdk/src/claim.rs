//! The Petal use claim: what a signing route tells Bloom it is about to sign.
//!
//! Bloom checks some of the claim against its own trusted context and takes
//! the rest on the Petal's word.
//!
//! - **Checked by Bloom:** `package_hash`, `route`, `operation_class`,
//!   `crypto_suite`, `payload_digest` and `ordered_hashes`. A wrong value only
//!   gets the signature refused, so [`petal_use_claim`] fills them in from the
//!   route context and the payloads.
//! - **Asserted by the Petal:** the debits, destinations and fee. Broker turns
//!   these into the approval's value limits, checks the destinations against
//!   the wallet's `allowed_destinations`, and shows them to the owner. An empty
//!   list is not a neutral default: it gives Broker nothing to limit or check
//!   and the owner nothing to read. So [`ClaimEffects`] has no default, and a
//!   claim that moves value must say what it moves.
//!
//! Broker compares a declared destination with the wallet policy by exact
//! string equality on both the chain and the destination. Declare each one in
//! exactly the form the owner's policy uses: a lowercase address never matches
//! a checksummed policy entry, and `evm-8453` never matches `base`.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Ctx, DispatchResponse, PayloadSignItem, error, payload_batch_digest, route_param};

/// Domain separator for the default claim nonce.
pub const CLAIM_NONCE_DOMAIN_V1: &[u8] = b"bloom.petal.claim-nonce.v1\0";

const U256_MAX: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";

/// The hash a signature is made over, named as Bloom's wire format names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoSuite {
    /// keccak256, what EVM chains verify.
    Secp256k1Keccak256Recoverable,
    /// SHA-256, for off-chain use only; no chain accepts it.
    Secp256k1Sha256Recoverable,
    /// The only suite a Solana wallet permits.
    Ed25519Message,
}

impl CryptoSuite {
    /// The wire name, also the `signature_algorithm` of a sign request.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Secp256k1Keccak256Recoverable => "secp256k1-keccak256-recoverable",
            Self::Secp256k1Sha256Recoverable => "secp256k1-sha256-recoverable",
            Self::Ed25519Message => "ed25519-message",
        }
    }
}

/// An asset on a chain, as Broker names it in value limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimAsset {
    pub chain: String,
    pub asset: String,
}

/// Value the signed payload takes from the wallet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimDebit {
    pub asset: ClaimAsset,
    /// Canonical decimal, in the asset's smallest unit.
    pub amount: String,
}

/// Where the signed payload sends value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimDestination {
    pub chain: String,
    pub destination: String,
}

/// The network fee the signed payload can spend. Broker refuses `None` for an
/// operation class it catalogs as fee-bearing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimFee {
    None,
    Fee {
        chain: String,
        asset: String,
        /// Canonical decimal, in the asset's smallest unit.
        amount: String,
    },
}

/// What a signing route declares its payload does. There is deliberately no
/// default: choose [`ClaimEffects::moves_nothing`] or
/// [`ClaimEffects::moves_value`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimEffects {
    moves_value: bool,
    debits: Vec<ClaimDebit>,
    destinations: Vec<ClaimDestination>,
    fee: ClaimFee,
}

impl ClaimEffects {
    /// The payload moves no value and pays no fee: for example a login
    /// message, or an order cancellation whose venue charges nothing. This is
    /// a statement about the payload, not a placeholder; use
    /// [`ClaimEffects::moves_value`] whenever value can leave the wallet.
    pub fn moves_nothing() -> Self {
        Self {
            moves_value: false,
            debits: Vec::new(),
            destinations: Vec::new(),
            fee: ClaimFee::None,
        }
    }

    /// Start a claim for a payload that moves value. Building it fails unless
    /// it declares at least one debit or a fee.
    pub fn moves_value() -> Self {
        Self {
            moves_value: true,
            ..Self::moves_nothing()
        }
    }

    /// Declare value the payload takes from the wallet, in the asset's
    /// smallest unit.
    pub fn debit(mut self, chain: &str, asset: &str, amount: &str) -> Self {
        self.debits.push(ClaimDebit {
            asset: ClaimAsset {
                chain: chain.to_owned(),
                asset: asset.to_owned(),
            },
            amount: amount.to_owned(),
        });
        self
    }

    /// Declare where the payload sends value. Use exactly the chain and
    /// destination strings the owner's policy allows; Broker compares them
    /// byte for byte.
    pub fn destination(mut self, chain: &str, destination: &str) -> Self {
        self.destinations.push(ClaimDestination {
            chain: chain.to_owned(),
            destination: destination.to_owned(),
        });
        self
    }

    /// Declare the network fee the payload can spend, in the asset's smallest
    /// unit. Declare the cap the payload allows, not an estimate.
    pub fn fee(mut self, chain: &str, asset: &str, amount: &str) -> Self {
        self.fee = ClaimFee::Fee {
            chain: chain.to_owned(),
            asset: asset.to_owned(),
            amount: amount.to_owned(),
        };
        self
    }

    pub fn debits(&self) -> &[ClaimDebit] {
        &self.debits
    }

    pub fn destinations(&self) -> &[ClaimDestination] {
        &self.destinations
    }

    pub fn declared_fee(&self) -> &ClaimFee {
        &self.fee
    }

    /// Check every value against Bloom's wire grammar, so a bad claim fails in
    /// the Petal with a clear message instead of at Broker after the owner has
    /// been asked to approve.
    pub fn validate(&self) -> Result<(), String> {
        if !self.moves_value {
            if !self.debits.is_empty()
                || !self.destinations.is_empty()
                || self.fee != ClaimFee::None
            {
                return Err(
                    "a claim declared as moving nothing cannot carry debits, destinations or a fee; use ClaimEffects::moves_value()"
                        .into(),
                );
            }
            return Ok(());
        }
        if self.debits.is_empty() && self.fee == ClaimFee::None {
            return Err("a value-moving claim must declare at least one debit or a fee".into());
        }
        for debit in &self.debits {
            validate_chain(&debit.asset.chain)?;
            validate_label("debit asset", &debit.asset.asset)?;
            validate_amount(&debit.amount)?;
        }
        for destination in &self.destinations {
            validate_chain(&destination.chain)?;
            validate_label("destination", &destination.destination)?;
        }
        if let ClaimFee::Fee {
            chain,
            asset,
            amount,
        } = &self.fee
        {
            validate_chain(chain)?;
            validate_label("fee asset", asset)?;
            validate_amount(amount)?;
        }
        Ok(())
    }
}

/// The claim nonce: derived from the claim's identity by default, so a retry
/// of the same payload presents the same claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimNonce {
    /// SHA-256 over a domain separator, the package hash, the route, the
    /// operation class and the payload digest, truncated to 16 bytes.
    Derived,
    /// A nonce the Petal chose, for a payload that must be claimable again as
    /// a distinct request.
    Explicit([u8; 16]),
}

#[derive(Serialize)]
struct WireClaim<'a> {
    package_hash: &'a str,
    route: &'a str,
    operation_class: &'a str,
    crypto_suite: &'static str,
    payload_digest: String,
    ordered_hashes: Vec<String>,
    declared_debits: &'a [ClaimDebit],
    declared_destinations: &'a [ClaimDestination],
    declared_fee: &'a ClaimFee,
    nonce: String,
    claim_assurance: WireAssurance,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireAssurance {
    MachineAsserted,
}

/// Build the canonical (RFC 8785) Petal use claim for `payloads`.
///
/// `package_hash` comes from the route context and the route id from Bloom's
/// trusted `bloom.route_id` parameter; `payload_digest` and `ordered_hashes`
/// come from the payloads. Pass the same `operation_class` and `suite` you put
/// in the sign request. The claim is machine-asserted: Broker shows it and
/// enforces limits and destinations from it, but no verifier proves it.
///
/// Errors: `-2` if the route is not running with Bloom's trusted route id,
/// `-3` for an invalid operation class, payload list or effects, and `-4` if
/// the context's package hash is malformed.
pub fn petal_use_claim(
    ctx: &Ctx,
    operation_class: &str,
    suite: CryptoSuite,
    payloads: &[PayloadSignItem],
    effects: &ClaimEffects,
    nonce: ClaimNonce,
) -> Result<Vec<u8>, DispatchResponse> {
    let route = route_param(ctx, "bloom.route_id")
        .ok_or_else(|| error(-2, "route is not running with Bloom's trusted route id"))?;
    if !is_lower_hex(&ctx.package_hash, 64) {
        return Err(error(-4, "route context carries a malformed package hash"));
    }
    validate_token("operation class", operation_class).map_err(|message| error(-3, message))?;
    effects.validate().map_err(|message| error(-3, message))?;
    let digest = payload_batch_digest(payloads).map_err(|err| error(-3, err.message()))?;
    let nonce = match nonce {
        ClaimNonce::Explicit(bytes) => bytes,
        ClaimNonce::Derived => derived_nonce(&ctx.package_hash, route, operation_class, &digest),
    };
    let claim = WireClaim {
        package_hash: &ctx.package_hash,
        route,
        operation_class,
        crypto_suite: suite.as_str(),
        payload_digest: to_hex(&digest),
        ordered_hashes: payloads
            .iter()
            .map(|item| to_hex(&item.claimed_hash))
            .collect(),
        declared_debits: &effects.debits,
        declared_destinations: &effects.destinations,
        declared_fee: &effects.fee,
        nonce: to_hex(&nonce),
        claim_assurance: WireAssurance::MachineAsserted,
    };
    serde_jcs::to_vec(&claim).map_err(|err| error(-4, format!("canonicalize claim: {err}")))
}

/// The default [`ClaimNonce::Derived`] value, exposed so a Petal can record or
/// test it.
pub fn derived_nonce(
    package_hash: &str,
    route: &str,
    operation_class: &str,
    payload_digest: &[u8; 32],
) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(CLAIM_NONCE_DOMAIN_V1);
    for part in [
        package_hash.as_bytes(),
        route.as_bytes(),
        operation_class.as_bytes(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.update(payload_digest);
    let full: [u8; 32] = hasher.finalize().into();
    let mut nonce = [0; 16];
    nonce.copy_from_slice(&full[..16]);
    nonce
}

#[derive(Deserialize)]
struct WireEffects {
    declared_debits: Vec<ClaimDebit>,
    declared_destinations: Vec<ClaimDestination>,
    declared_fee: ClaimFee,
}

/// Read the declared effects back out of an emitted claim, so a Petal's tests
/// can assert exactly what it declares.
pub fn claim_effects(claim_jcs: &[u8]) -> Result<ClaimEffects, String> {
    let wire: WireEffects = serde_json::from_slice::<serde_json::Value>(claim_jcs)
        .and_then(serde_json::from_value)
        .map_err(|err| format!("decode claim effects: {err}"))?;
    let moves_value = !wire.declared_debits.is_empty() || wire.declared_fee != ClaimFee::None;
    Ok(ClaimEffects {
        moves_value,
        debits: wire.declared_debits,
        destinations: wire.declared_destinations,
        fee: wire.declared_fee,
    })
}

fn validate_amount(amount: &str) -> Result<(), String> {
    let canonical = !amount.is_empty()
        && amount.bytes().all(|byte| byte.is_ascii_digit())
        && (amount == "0" || !amount.starts_with('0'));
    let in_range =
        amount.len() < U256_MAX.len() || (amount.len() == U256_MAX.len() && amount <= U256_MAX);
    if canonical && in_range {
        Ok(())
    } else {
        Err(format!(
            "amount {amount:?} must be a canonical decimal integer no greater than 2^256-1"
        ))
    }
}

fn validate_chain(chain: &str) -> Result<(), String> {
    validate_token("chain", chain)
}

/// Bloom's `Token`: 1-64 bytes, a lowercase first byte, then `[a-z0-9._/-]`.
fn validate_token(label: &str, value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let valid = (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'/' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} {value:?} must be a Bloom token: a lowercase letter, then up to 63 of [a-z0-9._/-]"
        ))
    }
}

fn validate_label(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(format!(
            "{label} must be 1-256 characters with no control characters"
        ))
    } else {
        Ok(())
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RouteIdentity;
    use crate::testing::RouteCtx;

    const PACKAGE: &str = "aa00000000000000000000000000000000000000000000000000000000000001";

    struct Confirm;
    impl RouteIdentity for Confirm {
        const PATH: &'static str = "transfers/[wallet]/[index]/confirm.json";
        const CANONICAL_PATH: &'static str = "transfers/[wallet]/[index]/confirm.json";
        const PARAMS: &'static [(&'static str, usize)] = &[("wallet", 1), ("index", 2)];
    }

    fn ctx() -> Ctx {
        RouteCtx::new("transfers/alice/0/confirm.json")
            .account("alice", 0)
            .param("bloom.route_id", "r000007")
            .package_hash(PACKAGE)
            .bind::<Confirm>()
    }

    fn payload(byte: u8) -> PayloadSignItem {
        PayloadSignItem {
            preimage: vec![byte; 3],
            claimed_hash: [byte; 32],
        }
    }

    fn send() -> ClaimEffects {
        ClaimEffects::moves_value()
            .debit("base", "native", "1000000000000000")
            .destination("base", "0xAbC0000000000000000000000000000000000001")
            .fee("base", "native", "21000000000000")
    }

    fn code(response: &DispatchResponse) -> i32 {
        match response {
            DispatchResponse::Error { code, .. } => *code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn claim(effects: &ClaimEffects) -> serde_json::Value {
        let jcs = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1)],
            effects,
            ClaimNonce::Derived,
        )
        .unwrap();
        serde_json::from_slice(&jcs).unwrap()
    }

    #[test]
    fn the_fields_bloom_checks_come_from_the_route_and_the_payloads() {
        let value = claim(&send());
        assert_eq!(value["package_hash"], PACKAGE);
        assert_eq!(value["route"], "r000007");
        assert_eq!(value["operation_class"], "transfer.send");
        assert_eq!(value["crypto_suite"], "secp256k1-keccak256-recoverable");
        let digest = payload_batch_digest(&[payload(1)]).unwrap();
        assert_eq!(value["payload_digest"], to_hex(&digest));
        assert_eq!(
            value["ordered_hashes"],
            serde_json::json!([to_hex(&[1; 32])])
        );
        assert_eq!(
            value["claim_assurance"],
            serde_json::json!({"kind": "machine_asserted"})
        );
    }

    #[test]
    fn a_claim_is_exact_canonical_json() {
        // A golden vector: changing the wire shape must change this test.
        let jcs = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Ed25519Message,
            &[payload(2)],
            &ClaimEffects::moves_nothing(),
            ClaimNonce::Explicit([7; 16]),
        )
        .unwrap();
        let digest = to_hex(&payload_batch_digest(&[payload(2)]).unwrap());
        let expected = format!(
            concat!(
                r#"{{"claim_assurance":{{"kind":"machine_asserted"}},"crypto_suite":"ed25519-message","#,
                r#""declared_debits":[],"declared_destinations":[],"declared_fee":{{"kind":"none"}},"#,
                r#""nonce":"07070707070707070707070707070707","operation_class":"transfer.send","#,
                r#""ordered_hashes":["{hashes}"],"package_hash":"{package}","payload_digest":"{digest}","#,
                r#""route":"r000007"}}"#
            ),
            hashes = to_hex(&[2; 32]),
            package = PACKAGE,
            digest = digest,
        );
        assert_eq!(String::from_utf8(jcs).unwrap(), expected);
    }

    #[test]
    fn declared_effects_reach_the_claim_unchanged() {
        let value = claim(&send());
        assert_eq!(
            value["declared_debits"],
            serde_json::json!([{"asset": {"chain": "base", "asset": "native"}, "amount": "1000000000000000"}])
        );
        // Kept byte for byte: Broker compares destinations exactly.
        assert_eq!(
            value["declared_destinations"],
            serde_json::json!([{"chain": "base", "destination": "0xAbC0000000000000000000000000000000000001"}])
        );
        assert_eq!(
            value["declared_fee"],
            serde_json::json!({"kind": "fee", "chain": "base", "asset": "native", "amount": "21000000000000"})
        );
    }

    #[test]
    fn a_value_claim_must_say_what_it_moves() {
        // The empty claim safe and polymarket ship today is refused unless it
        // is declared as moving nothing.
        let empty = ClaimEffects::moves_value().destination("base", "0x01");
        assert!(
            empty
                .validate()
                .unwrap_err()
                .contains("at least one debit or a fee")
        );
        let refused = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1)],
            &empty,
            ClaimNonce::Derived,
        )
        .unwrap_err();
        assert_eq!(code(&refused), -3);
        // A fee alone is enough: a contract call that moves no token still pays.
        assert!(
            ClaimEffects::moves_value()
                .fee("base", "native", "1")
                .validate()
                .is_ok()
        );
        assert!(ClaimEffects::moves_nothing().validate().is_ok());
    }

    #[test]
    fn a_claim_declared_as_moving_nothing_cannot_carry_effects() {
        for effects in [
            ClaimEffects::moves_nothing().debit("base", "native", "1"),
            ClaimEffects::moves_nothing().destination("base", "0x01"),
            ClaimEffects::moves_nothing().fee("base", "native", "1"),
        ] {
            assert!(effects.validate().is_err(), "{effects:?}");
        }
    }

    #[test]
    fn amounts_and_chains_follow_bloom_wire_grammar() {
        for amount in ["0", "1", U256_MAX] {
            assert!(validate_amount(amount).is_ok(), "{amount}");
        }
        let over = "115792089237316195423570985008687907853269984665640564039457584007913129639936";
        for amount in [
            "",
            "01",
            "-1",
            "1.5",
            "1e18",
            " 1",
            "0x10",
            over,
            &"9".repeat(79),
        ] {
            assert!(validate_amount(amount).is_err(), "{amount:?}");
        }
        for chain in ["base", "evm-8453", "solana-mainnet", "a"] {
            assert!(validate_chain(chain).is_ok(), "{chain}");
        }
        for chain in ["", "Base", "8453", "-base", "base chain", &"a".repeat(65)] {
            assert!(validate_chain(chain).is_err(), "{chain:?}");
        }
        assert!(
            ClaimEffects::moves_value()
                .debit("base", "native", "1.5")
                .validate()
                .is_err()
        );
        assert!(
            ClaimEffects::moves_value()
                .fee("base", "native", "1")
                .destination("base", "")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn the_route_context_must_be_bloom_s() {
        let effects = ClaimEffects::moves_nothing();
        let build = |ctx: &Ctx, class: &str, payloads: &[PayloadSignItem]| {
            petal_use_claim(
                ctx,
                class,
                CryptoSuite::Ed25519Message,
                payloads,
                &effects,
                ClaimNonce::Derived,
            )
            .unwrap_err()
        };
        let no_route = RouteCtx::new("transfers/alice/0/confirm.json")
            .account("alice", 0)
            .package_hash(PACKAGE)
            .bind::<Confirm>();
        assert_eq!(code(&build(&no_route, "transfer.send", &[payload(1)])), -2);
        let bad_package = RouteCtx::new("transfers/alice/0/confirm.json")
            .param("bloom.route_id", "r000007")
            .bind::<Confirm>();
        assert_eq!(
            code(&build(&bad_package, "transfer.send", &[payload(1)])),
            -4
        );
        assert_eq!(code(&build(&ctx(), "Transfer.Send", &[payload(1)])), -3);
        assert_eq!(code(&build(&ctx(), "transfer.send", &[])), -3);
    }

    #[test]
    fn the_derived_nonce_is_stable_and_separates_claims() {
        let digest = payload_batch_digest(&[payload(1)]).unwrap();
        let other = payload_batch_digest(&[payload(2)]).unwrap();
        let nonce = derived_nonce(PACKAGE, "r000007", "transfer.send", &digest);
        assert_eq!(
            nonce,
            derived_nonce(PACKAGE, "r000007", "transfer.send", &digest)
        );
        assert_ne!(
            nonce,
            derived_nonce(PACKAGE, "r000007", "transfer.send", &other)
        );
        assert_ne!(
            nonce,
            derived_nonce(PACKAGE, "r000008", "transfer.send", &digest)
        );
        assert_ne!(
            nonce,
            derived_nonce(PACKAGE, "r000007", "transfer.cancel", &digest)
        );
        // Length-prefixed, so moving a byte between fields changes the nonce.
        assert_ne!(
            derived_nonce(PACKAGE, "r00000", "7transfer.send", &digest),
            nonce
        );
        let value = claim(&send());
        assert_eq!(value["nonce"], to_hex(&nonce));
    }

    #[test]
    fn a_petal_can_assert_the_effects_it_emitted() {
        let jcs = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1)],
            &send(),
            ClaimNonce::Derived,
        )
        .unwrap();
        assert_eq!(claim_effects(&jcs).unwrap(), send());
        assert!(claim_effects(b"{}").is_err());
    }

    #[test]
    fn the_suite_names_match_the_wire() {
        assert_eq!(
            CryptoSuite::Secp256k1Keccak256Recoverable.as_str(),
            "secp256k1-keccak256-recoverable"
        );
        assert_eq!(
            CryptoSuite::Secp256k1Sha256Recoverable.as_str(),
            "secp256k1-sha256-recoverable"
        );
        assert_eq!(CryptoSuite::Ed25519Message.as_str(), "ed25519-message");
    }
}
