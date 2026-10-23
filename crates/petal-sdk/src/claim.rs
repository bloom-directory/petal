//! The Petal use claim: what a signing route tells Bloom it is about to sign.
//!
//! Bloom checks some of the claim against its own trusted context and takes
//! the rest on the Petal's word.
//!
//! - **Checked by Bloom:** `package_hash`, `route`, `crypto_suite`,
//!   `payload_digest` and `ordered_hashes` against the context and the sign
//!   request, and `operation_class` against the classes cataloged for the
//!   route. Bloom compares the claim's class with the sign request's class on
//!   the batch and reusable paths, but not on an exact single-payload sign. A
//!   wrong value only gets the signature refused, so [`petal_use_claim`] fills
//!   them in from the route context and the payloads, and [`PetalClaim`]
//!   builds the sign request from the same class, suite and payloads so the
//!   two cannot disagree.
//! - **Asserted by the Petal:** the debits, destinations and fee. Bloom derives
//!   the approval's value limits from the debits and fee, and Broker enforces
//!   them. Broker checks each declared destination against the wallet's
//!   `allowed_destinations`, and shows all of it to the owner. Broker checks
//!   only the destinations a claim names, so a debit with no destination
//!   passes the destination policy unchecked. An empty list is not a neutral
//!   default: it gives Broker nothing to limit or check and the owner nothing
//!   to read. So [`ClaimEffects`] has no default, a claim that moves value
//!   must say what it moves, and a debit must say where it goes.
//!
//! A debit is the most the signed payload lets leave the wallet in that
//! asset, not just the principal: a venue, taker, protocol, builder or relayer
//! fee pulled together with the principal belongs in the same debit, declared
//! to the same destination; when such a fee goes to a different recipient,
//! declare that recipient as a destination too, since Broker checks only the
//! destinations a claim names. Broker accounts only declared values, so it
//! cannot detect an under-declared debit. The fee is only the network fee, in
//! the fee asset Bloom catalogs for the operation class, and Broker refuses
//! any declared fee on current Bloom (see [`ClaimFee`]).
//!
//! Broker matches a declared destination's chain against the wallet policy
//! exactly: `evm-8453` never matches `base`. On a chain whose address format
//! it knows, it then compares the destination by the address it decodes to,
//! not by its spelling: an EVM address matches in any letter case, because
//! case is only the EIP-55 checksum, while a Solana address stays
//! case-sensitive, a bech32 address keeps its prefix and a Bitcoin address
//! compares by its output script. A destination on any other chain, or one
//! that does not decode, is compared by its exact string. Brokers before
//! chain-aware destination matching (bloom-directory/bloom-broker#80) compare
//! every destination by its exact string, so declaring each one in exactly the
//! form the owner's policy uses stays the choice that works on every Broker.
//!
//! Every effect check runs after the owner has approved, so a claim Broker
//! refuses costs the owner a wasted approval. [`ClaimEffects::validate`]
//! catches what the SDK can see; it cannot see the wallet's policy or how
//! Bloom catalogs the operation class.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    Ctx, DispatchResponse, PayloadBatchSignRequest, PayloadSignItem, PayloadSignRequest,
    SignSelector, error, payload_batch_digest, route_param, validate_payload_batch,
};

/// Domain separator for the SDK's default claim nonce. Bloom does not check
/// the nonce; it is the SDK's convention for retry identity.
pub(crate) const CLAIM_NONCE_DOMAIN_V1: &[u8] = b"bloom.petal.claim-nonce.v1\0";

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
    /// Every suite. A suite missing here stops decoding, and the compiler
    /// cannot see this list is complete; the match after this impl makes a
    /// new variant fail to build until it is named there.
    const ALL: [Self; 3] = [
        Self::Secp256k1Keccak256Recoverable,
        Self::Secp256k1Sha256Recoverable,
        Self::Ed25519Message,
    ];

    /// The wire name, also the `signature_algorithm` of a sign request.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Secp256k1Keccak256Recoverable => "secp256k1-keccak256-recoverable",
            Self::Secp256k1Sha256Recoverable => "secp256k1-sha256-recoverable",
            Self::Ed25519Message => "ed25519-message",
        }
    }

    /// The suite whose [`CryptoSuite::as_str`] is `name`, exactly.
    fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|suite| suite.as_str() == name)
    }
}

// A new suite fails to build here until this match names it. Add it to
// `CryptoSuite::ALL` as well, or a claim naming it stops decoding.
const _: fn(CryptoSuite) = |suite| match suite {
    CryptoSuite::Secp256k1Keccak256Recoverable
    | CryptoSuite::Secp256k1Sha256Recoverable
    | CryptoSuite::Ed25519Message => (),
};

/// An asset on a chain, as Broker names it in value limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimAsset {
    pub chain: String,
    pub asset: String,
}

impl ClaimAsset {
    /// An asset on a chain, in the exact strings Bloom's catalog and the
    /// wallet's policy use.
    pub fn new(chain: &str, asset: &str) -> Self {
        Self {
            chain: chain.to_owned(),
            asset: asset.to_owned(),
        }
    }

    /// The chain's gas asset, which Bloom names `native`.
    pub fn native(chain: &str) -> Self {
        Self::new(chain, "native")
    }
}

/// The most the signed payload lets leave the wallet in one asset.
///
/// That includes every venue, taker, protocol, builder or relayer fee pulled
/// together with the principal, declared to the same destination: an exchange
/// that takes its taker fee from the wallet at match time takes more than the
/// order's principal. When such a fee goes to a different recipient, declare
/// that recipient as a destination too, since Broker checks only the
/// destinations a claim names. Broker accounts only declared values, so it
/// cannot detect an under-declared debit.
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

/// The network fee the signed payload can spend, in the fee asset Bloom
/// catalogs for the operation class. It is never a venue fee: a venue, taker,
/// protocol, builder or relayer fee belongs in the debit (see [`ClaimDebit`]).
///
/// Whether a class is fee-bearing is decided by Bloom's catalog, not by the
/// claim. Broker refuses, after the owner has approved:
///
/// - `FEE_REQUIRED`: `None` on a fee-bearing class;
/// - `FEE_NOT_ALLOWED`: a fee on a class with no fee asset;
/// - `FEE_ASSET_MISMATCH`: a fee whose chain or asset differs from the
///   catalog's, compared byte for byte.
///
/// Current Bloom catalogs no fee asset for any Petal operation class, so
/// Broker refuses every declared fee with `FEE_NOT_ALLOWED` after the owner
/// has approved, and a Petal claim must declare `None` until Bloom lets a
/// Petal declare its fee asset (bloom#276). [`ClaimEffects::validate`] still
/// accepts a fee, even as a claim's only effect, so the type is usable once
/// bloom#276 lands; Petals pin the SDK, so a later release can tighten that.
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
    fees_declared: usize,
    debits_without_destination: bool,
}

impl ClaimEffects {
    /// The payload moves no value, authorizes no one and pays no fee: for
    /// example a login message, or an order cancellation whose venue charges
    /// nothing. Effects are relative to the signing wallet.
    ///
    /// This is a statement about the payload, not a placeholder, and the SDK
    /// cannot check it: it emits the same bytes as the empty claim. Use
    /// [`ClaimEffects::moves_value`] whenever value can leave the wallet, or
    /// someone gains the right to take it.
    pub fn moves_nothing() -> Self {
        Self {
            moves_value: false,
            debits: Vec::new(),
            destinations: Vec::new(),
            fee: ClaimFee::None,
            fees_declared: 0,
            debits_without_destination: false,
        }
    }

    /// Start a claim for a payload that moves value, or lets someone move it.
    ///
    /// It must declare at least one non-zero debit or a destination. A
    /// payload that only authorizes, such as a token allowance or an operator
    /// approval, declares the spender as a destination and no debit, so the
    /// wallet's destination policy still applies to it. Every debit needs a
    /// destination unless [`ClaimEffects::debits_without_destination`] says
    /// otherwise.
    pub fn moves_value() -> Self {
        Self {
            moves_value: true,
            ..Self::moves_nothing()
        }
    }

    /// Declare the most the payload lets leave the wallet in `asset`, in the
    /// asset's smallest unit. Include every venue, taker, protocol, builder or
    /// relayer fee pulled together with the principal, declared to the same
    /// destination; when such a fee goes to a different recipient, declare
    /// that recipient with [`ClaimEffects::destination`] too, since Broker
    /// checks only the destinations a claim names. [`ClaimEffects::fee`] is
    /// only the network fee. Broker accounts only declared values, so it
    /// cannot detect an under-declared debit. The amount must not be zero;
    /// leave out a debit that takes nothing.
    pub fn debit(mut self, asset: &ClaimAsset, amount: &str) -> Self {
        self.debits.push(ClaimDebit {
            asset: asset.clone(),
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

    /// Declare the network fee the payload can spend, in the smallest unit of
    /// the fee asset Bloom catalogs for the operation class; never a venue
    /// fee, which belongs in the debit. Current Bloom catalogs no fee asset
    /// for any Petal class, so Broker refuses a declared fee with
    /// `FEE_NOT_ALLOWED` after the owner approves, until bloom#276 (see
    /// [`ClaimFee`]). Declare the cap the payload allows, not an estimate. A
    /// claim carries at most one fee, so a second call fails validation.
    pub fn fee(mut self, asset: &ClaimAsset, amount: &str) -> Self {
        self.fee = ClaimFee::Fee {
            chain: asset.chain.clone(),
            asset: asset.asset.clone(),
            amount: amount.to_owned(),
        };
        self.fees_declared += 1;
        self
    }

    /// Declare that the debits go to no address the owner's policy can name,
    /// so Broker's destination check cannot apply to them: for example value
    /// that stays inside a venue account, or a fee a venue keeps. Broker still
    /// enforces the debits as value limits. Use it only when that is true;
    /// whenever the payload names a recipient, declare it with
    /// [`ClaimEffects::destination`] instead.
    pub fn debits_without_destination(mut self) -> Self {
        self.debits_without_destination = true;
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
                || self.debits_without_destination
            {
                return Err(
                    "a claim declared as moving nothing cannot carry debits, destinations or a fee; use ClaimEffects::moves_value()"
                        .into(),
                );
            }
            return Ok(());
        }
        if self.fees_declared > 1 {
            return Err("a claim declares at most one fee".into());
        }
        for debit in &self.debits {
            validate_chain(&debit.asset.chain)?;
            validate_label("debit asset", &debit.asset.asset)?;
            validate_amount(&debit.amount)?;
            if debit.amount == "0" {
                return Err(format!(
                    "a debit of {} on {} is zero; leave out a debit that takes nothing",
                    debit.asset.asset, debit.asset.chain
                ));
            }
        }
        for destination in &self.destinations {
            validate_chain(&destination.chain)?;
            validate_label("destination", &destination.destination)?;
        }
        let mut fee_moves_value = false;
        if let ClaimFee::Fee {
            chain,
            asset,
            amount,
        } = &self.fee
        {
            validate_chain(chain)?;
            validate_label("fee asset", asset)?;
            validate_amount(amount)?;
            fee_moves_value = amount != "0";
        }
        if self.debits.is_empty() && self.destinations.is_empty() && !fee_moves_value {
            return Err(
                "a value-moving claim must declare a non-zero debit or a destination".into(),
            );
        }
        if !self.debits.is_empty()
            && self.destinations.is_empty()
            && !self.debits_without_destination
        {
            return Err(
                "a claim with debits must declare where the value goes, so the wallet's allowed_destinations can check it; if no policy address applies, say so with debits_without_destination()"
                    .into(),
            );
        }
        if self.debits_without_destination && self.debits.is_empty() {
            return Err("debits_without_destination() needs at least one debit".into());
        }
        self.validate_totals()
    }

    /// Bloom sums the debits and fee per asset and refuses a total above
    /// 2^256-1, so check it here rather than after the owner approves.
    fn validate_totals(&self) -> Result<(), String> {
        let mut totals: Vec<(&str, &str, String)> = Vec::new();
        let fee = match &self.fee {
            ClaimFee::Fee {
                chain,
                asset,
                amount,
            } => Some((chain.as_str(), asset.as_str(), amount.as_str())),
            ClaimFee::None => None,
        };
        let entries = self
            .debits
            .iter()
            .map(|debit| {
                (
                    debit.asset.chain.as_str(),
                    debit.asset.asset.as_str(),
                    debit.amount.as_str(),
                )
            })
            .chain(fee);
        for (chain, asset, amount) in entries {
            match totals
                .iter_mut()
                .find(|(c, a, _)| *c == chain && *a == asset)
            {
                Some((_, _, total)) => *total = add_decimal(total, amount),
                None => totals.push((chain, asset, amount.to_owned())),
            }
        }
        for (chain, asset, total) in &totals {
            if validate_amount(total).is_err() {
                return Err(format!(
                    "the declared {asset} on {chain}, debits and fee together, exceeds 2^256-1"
                ));
            }
        }
        Ok(())
    }
}

/// The claim nonce.
///
/// Bloom keys an exact-signing request, its state and the owner's approval on
/// the whole canonical claim, nonce included, and pins the request's `action`
/// and `advisory` digests to it. So a retry, including the call that collects
/// the signature after the owner approves, must present the same claim bytes:
/// the same nonce, the same effects and byte-identical `action` and
/// `advisory`. A changed claim is a new request with a new approval, and a
/// changed `action` or `advisory` under the same claim is refused. Bloom does
/// not otherwise check the nonce.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimNonce {
    /// SHA-256 over a domain separator, the package hash, the route, the
    /// operation class and the payload digest, truncated to 16 bytes. The same
    /// payload gives the same nonce, so a retry reproduces the claim as long
    /// as its effects, `action` and `advisory` do not change: keep timestamps
    /// and re-fetched quotes out of them.
    Derived,
    /// A nonce the Petal chose, for a payload that must be claimable again as
    /// a distinct request. Persist it with the operation record and present
    /// the same nonce on every retry, or the owner's approval will not match.
    Explicit([u8; 16]),
}

/// The claim as it goes on the wire. [`ClaimEffects::from_claim_jcs`] decodes
/// into the same type, so the two cannot disagree on the fields or their
/// names.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireClaim {
    package_hash: String,
    route: String,
    operation_class: String,
    crypto_suite: String,
    payload_digest: String,
    ordered_hashes: Vec<String>,
    declared_debits: Vec<ClaimDebit>,
    declared_destinations: Vec<ClaimDestination>,
    declared_fee: ClaimFee,
    nonce: String,
    claim_assurance: WireAssurance,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum WireAssurance {
    MachineAsserted,
}

/// Who is claiming: the package and route Bloom dispatched. Taking it apart
/// from the [`Ctx`] lets a Petal that passes context pieces down its call
/// stack, or tests without a route context, build claims too.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimOrigin {
    package_hash: String,
    route: String,
}

impl ClaimOrigin {
    /// Read the package hash and Bloom's trusted `bloom.route_id` from the
    /// route context.
    ///
    /// Errors: `-2` if the route is not running with Bloom's trusted route id,
    /// which includes a call from `metadata()`, where Bloom passes no `bloom.*`
    /// parameters, and `-4` if the context's package hash or route id is
    /// malformed.
    pub fn from_ctx(ctx: &Ctx) -> Result<Self, DispatchResponse> {
        let route = route_param(ctx, "bloom.route_id")
            .ok_or_else(|| error(-2, "route is not running with Bloom's trusted route id"))?;
        Self::new(&ctx.package_hash, route)
            .map_err(|message| error(-4, format!("route context: {message}")))
    }

    /// A package hash (64 lowercase hex digits) and a route id, as Bloom
    /// supplies them.
    pub fn new(package_hash: &str, route: &str) -> Result<Self, String> {
        if !is_lower_hex(package_hash, 64) {
            return Err("package hash must be 64 lowercase hex digits".into());
        }
        validate_token("route id", route)?;
        Ok(Self {
            package_hash: package_hash.to_owned(),
            route: route.to_owned(),
        })
    }

    pub fn package_hash(&self) -> &str {
        &self.package_hash
    }

    pub fn route(&self) -> &str {
        &self.route
    }

    /// Build the canonical (RFC 8785) Petal use claim for `payloads`.
    ///
    /// `payload_digest` and `ordered_hashes` come from the payloads. The claim
    /// is machine-asserted: Broker shows it and enforces limits and
    /// destinations from it, but no verifier proves it. Build the sign request
    /// from the returned [`PetalClaim`], so it carries the same class, suite
    /// and payloads.
    ///
    /// Errors: `-3` for an invalid operation class, payload list or effects.
    pub fn claim(
        &self,
        operation_class: &str,
        suite: CryptoSuite,
        payloads: &[PayloadSignItem],
        effects: &ClaimEffects,
        nonce: ClaimNonce,
    ) -> Result<PetalClaim, DispatchResponse> {
        validate_operation_class(operation_class).map_err(|message| error(-3, message))?;
        effects.validate().map_err(|message| error(-3, message))?;
        if payloads.len() > 1 {
            validate_payload_batch(payloads).map_err(|err| error(-3, err.message()))?;
        }
        let digest = payload_batch_digest(payloads).map_err(|err| error(-3, err.message()))?;
        let nonce = match nonce {
            ClaimNonce::Explicit(bytes) => bytes,
            ClaimNonce::Derived => {
                derived_nonce(&self.package_hash, &self.route, operation_class, &digest)
            }
        };
        let claim = WireClaim {
            package_hash: self.package_hash.clone(),
            route: self.route.clone(),
            operation_class: operation_class.to_owned(),
            crypto_suite: suite.as_str().to_owned(),
            payload_digest: to_hex(&digest),
            ordered_hashes: payloads
                .iter()
                .map(|item| to_hex(&item.claimed_hash))
                .collect(),
            declared_debits: effects.debits.clone(),
            declared_destinations: effects.destinations.clone(),
            declared_fee: effects.fee.clone(),
            nonce: to_hex(&nonce),
            claim_assurance: WireAssurance::MachineAsserted,
        };
        let jcs = serde_jcs::to_vec(&claim)
            .map_err(|err| error(-4, format!("canonicalize claim: {err}")))?;
        Ok(PetalClaim {
            jcs,
            operation_class: operation_class.to_owned(),
            suite,
            payloads: payloads.to_vec(),
        })
    }
}

/// Build the canonical (RFC 8785) Petal use claim for `payloads`, for the
/// route `ctx` dispatched. See [`ClaimOrigin::from_ctx`] and
/// [`ClaimOrigin::claim`].
///
/// Errors: `-2` if the route is not running with Bloom's trusted route id,
/// `-3` for an invalid operation class, payload list or effects, and `-4` if
/// the context's package hash or route id is malformed.
pub fn petal_use_claim(
    ctx: &Ctx,
    operation_class: &str,
    suite: CryptoSuite,
    payloads: &[PayloadSignItem],
    effects: &ClaimEffects,
    nonce: ClaimNonce,
) -> Result<PetalClaim, DispatchResponse> {
    ClaimOrigin::from_ctx(ctx)?.claim(operation_class, suite, payloads, effects, nonce)
}

/// A built claim, together with the class, suite and payloads it covers.
///
/// Build the sign request from it rather than by hand: Bloom does not compare
/// the claim's class with the request's on an exact single-payload sign, so a
/// request that names a different class would be signed under the claim's
/// class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PetalClaim {
    jcs: Vec<u8>,
    operation_class: String,
    suite: CryptoSuite,
    payloads: Vec<PayloadSignItem>,
}

impl PetalClaim {
    /// The canonical claim bytes.
    pub fn jcs(&self) -> &[u8] {
        &self.jcs
    }

    pub fn into_jcs(self) -> Vec<u8> {
        self.jcs
    }

    pub fn operation_class(&self) -> &str {
        &self.operation_class
    }

    pub fn suite(&self) -> CryptoSuite {
        self.suite
    }

    pub fn payloads(&self) -> &[PayloadSignItem] {
        &self.payloads
    }

    /// The sign request for a single-payload claim. Set `action`, `advisory`
    /// and `approval_hint` on it as the route needs; keep them byte-identical
    /// across retries (see [`ClaimNonce`]).
    ///
    /// Errors: `-3` if the claim covers more than one payload; use
    /// [`PetalClaim::batch_request`].
    pub fn sign_request(
        &self,
        wallet: &str,
        selector: SignSelector,
    ) -> Result<PayloadSignRequest, DispatchResponse> {
        let [item] = self.payloads.as_slice() else {
            return Err(error(
                -3,
                "a claim over several payloads needs a batch sign request",
            ));
        };
        Ok(PayloadSignRequest {
            wallet: wallet.to_owned(),
            preimage: item.preimage.clone(),
            claimed_hash: item.claimed_hash,
            signature_algorithm: self.suite.as_str().to_owned(),
            operation_class: self.operation_class.clone(),
            petal_use_claim_jcs: self.jcs.clone(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: None,
            advisory: None,
            selector,
            key_ref_jcs: None,
        })
    }

    /// The batch sign request for this claim, over the same payloads in the
    /// same order. The same retry rules apply as for
    /// [`PetalClaim::sign_request`].
    pub fn batch_request(&self, wallet: &str, selector: SignSelector) -> PayloadBatchSignRequest {
        PayloadBatchSignRequest {
            wallet: wallet.to_owned(),
            payloads: self.payloads.clone(),
            signature_algorithm: self.suite.as_str().to_owned(),
            operation_class: self.operation_class.clone(),
            petal_use_claim_jcs: self.jcs.clone(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: None,
            advisory: None,
            selector,
            key_ref_jcs: None,
        }
    }
}

/// The [`ClaimNonce::Derived`] value. A Petal that needs it reads the `nonce`
/// field of the claim it built.
pub(crate) fn derived_nonce(
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

impl ClaimEffects {
    /// Read the declared effects back out of an emitted claim, so a Petal's tests
    /// can assert exactly what it declares.
    ///
    /// It accepts a claim only if it has:
    ///
    /// - every claim field and no other, in canonical (RFC 8785) form, with a
    ///   `machine_asserted` assurance;
    /// - a `package_hash` and `route` that [`ClaimOrigin::new`] accepts;
    /// - an `operation_class` that [`ClaimOrigin::claim`] accepts, a Bloom
    ///   token;
    /// - a `crypto_suite` that is exactly the wire name of a [`CryptoSuite`];
    /// - a `payload_digest` of 64 and a `nonce` of 32 lowercase hex digits,
    ///   and one or more `ordered_hashes` of 64 lowercase hex digits each;
    /// - effects that pass [`ClaimEffects::validate`].
    ///
    /// The origin, class and effects go through the builder's own validators,
    /// and the suite is compared with [`CryptoSuite::as_str`] of each suite
    /// the SDK lists. Without the payloads it cannot recompute the digest, the
    /// hashes or a derived nonce, so it checks only their form, and it does not
    /// bound the number of hashes. A claim with any effect decodes as
    /// [`ClaimEffects::moves_value`], and one with none as
    /// [`ClaimEffects::moves_nothing`].
    pub fn from_claim_jcs(claim_jcs: &[u8]) -> Result<Self, String> {
        let wire: WireClaim =
            serde_json::from_slice(claim_jcs).map_err(|err| format!("decode claim: {err}"))?;
        let canonical =
            serde_jcs::to_vec(&wire).map_err(|err| format!("canonicalize claim: {err}"))?;
        if canonical != claim_jcs {
            return Err("claim is not in canonical (RFC 8785) form".into());
        }
        ClaimOrigin::new(&wire.package_hash, &wire.route)
            .map_err(|message| format!("claim {message}"))?;
        validate_operation_class(&wire.operation_class)
            .map_err(|message| format!("claim {message}"))?;
        if CryptoSuite::from_wire(&wire.crypto_suite).is_none() {
            return Err(format!(
                "claim crypto_suite {:?} is not a supported suite",
                wire.crypto_suite
            ));
        }
        let hex_fields = [
            ("payload_digest", wire.payload_digest.as_str(), 64),
            ("nonce", wire.nonce.as_str(), 32),
        ];
        for (field, value, length) in hex_fields {
            if !is_lower_hex(value, length) {
                return Err(format!(
                    "claim {field} must be {length} lowercase hex digits"
                ));
            }
        }
        if wire.ordered_hashes.is_empty()
            || !wire
                .ordered_hashes
                .iter()
                .all(|hash| is_lower_hex(hash, 64))
        {
            return Err(
                "claim ordered_hashes must list one or more 64-digit lowercase hex hashes".into(),
            );
        }
        let moves_value = !wire.declared_debits.is_empty()
            || !wire.declared_destinations.is_empty()
            || wire.declared_fee != ClaimFee::None;
        let fees_declared = usize::from(wire.declared_fee != ClaimFee::None);
        let debits_without_destination =
            !wire.declared_debits.is_empty() && wire.declared_destinations.is_empty();
        let effects = Self {
            moves_value,
            debits: wire.declared_debits,
            destinations: wire.declared_destinations,
            fee: wire.declared_fee,
            fees_declared,
            debits_without_destination,
        };
        effects.validate()?;
        Ok(effects)
    }
}

/// Add two canonical decimal integers. The result can exceed 2^256-1; the
/// caller checks it.
fn add_decimal(left: &str, right: &str) -> String {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let mut digits = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut carry = 0;
    for place in 0..left.len().max(right.len()) {
        let digit = |number: &[u8]| {
            number
                .len()
                .checked_sub(place + 1)
                .map_or(0, |index| number[index] - b'0')
        };
        let sum = digit(left) + digit(right) + carry;
        digits.push(b'0' + sum % 10);
        carry = sum / 10;
    }
    if carry > 0 {
        digits.push(b'0' + carry);
    }
    digits.reverse();
    String::from_utf8(digits).expect("decimal digits are ASCII")
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

fn validate_operation_class(operation_class: &str) -> Result<(), String> {
    validate_token("operation class", operation_class)
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

/// Broker compares assets, and any destination it does not decode, byte for
/// byte, so a label is printable ASCII with no spaces: invisible, bidi and
/// look-alike characters would make a value that reads like the policy entry
/// and never matches it.
/// Addresses, base58, venue ids and `native` all fit.
fn validate_label(label: &str, value: &str) -> Result<(), String> {
    if (1..=256).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_graphic()) {
        Ok(())
    } else {
        Err(format!(
            "{label} must be 1-256 bytes of printable ASCII with no spaces"
        ))
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
            .route_id("r000007")
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
            .debit(&ClaimAsset::new("base", "native"), "1000000000000000")
            .destination("base", "0xAbC0000000000000000000000000000000000001")
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
        .unwrap()
        .into_jcs();
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
        // Golden vectors, written out in full rather than computed with the
        // SDK's own digest code, so a change to the wire shape, the batch
        // digest domain or its length prefixes fails here. The digests were
        // computed independently: SHA-256 over the domain, the u64 big-endian
        // item count, then each preimage's u64 big-endian length and bytes.
        let one = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Ed25519Message,
            &[payload(2)],
            &ClaimEffects::moves_nothing(),
            ClaimNonce::Explicit([7; 16]),
        )
        .unwrap();
        assert_eq!(
            core::str::from_utf8(one.jcs()).unwrap(),
            concat!(
                r#"{"claim_assurance":{"kind":"machine_asserted"},"crypto_suite":"ed25519-message","declared_"#,
                r#"debits":[],"declared_destinations":[],"declared_fee":{"kind":"none"},"nonce":"070707070707"#,
                r#"07070707070707070707","operation_class":"transfer.send","ordered_hashes":["020202020202020"#,
                r#"2020202020202020202020202020202020202020202020202"],"package_hash":"aa00000000000000000000"#,
                r#"000000000000000000000000000000000000000001","payload_digest":"3804fefd8cc0b4ade2e77decf718"#,
                r#"dee9246cdbae9bf8f3fa0ddbea0e0046af2d","route":"r000007"}"#,
            )
        );

        // Several payloads keep their order, and several effects keep theirs.
        let effects = ClaimEffects::moves_value()
            .debit(&ClaimAsset::new("base", "usdc"), "5")
            .debit(&ClaimAsset::native("base"), "7")
            .destination("base", "0xAbC0000000000000000000000000000000000001")
            .destination("base", "0xAbC0000000000000000000000000000000000002")
            .fee(&ClaimAsset::native("base"), "9");
        let two = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1), payload(2)],
            &effects,
            ClaimNonce::Explicit([10; 16]),
        )
        .unwrap();
        assert_eq!(
            core::str::from_utf8(two.jcs()).unwrap(),
            concat!(
                r#"{"claim_assurance":{"kind":"machine_asserted"},"crypto_suite":"secp256k1-keccak256-recover"#,
                r#"able","declared_debits":[{"amount":"5","asset":{"asset":"usdc","chain":"base"}},{"amount":"#,
                r#""7","asset":{"asset":"native","chain":"base"}}],"declared_destinations":[{"chain":"base",""#,
                r#"destination":"0xAbC0000000000000000000000000000000000001"},{"chain":"base","destination":""#,
                r#"0xAbC0000000000000000000000000000000000002"}],"declared_fee":{"amount":"9","asset":"native"#,
                r#"","chain":"base","kind":"fee"},"nonce":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","operation_class"#,
                r#"":"transfer.send","ordered_hashes":["01010101010101010101010101010101010101010101010101010"#,
                r#"10101010101","0202020202020202020202020202020202020202020202020202020202020202"],"package_"#,
                r#"hash":"aa00000000000000000000000000000000000000000000000000000000000001","payload_digest":"#,
                r#""80986b51a09b6ecdf8e5c0b7722785da768339d46743728ab5eff3b108296db7","route":"r000007"}"#,
            )
        );
        // It decodes back to the same effects, fee included.
        assert_eq!(ClaimEffects::from_claim_jcs(two.jcs()).unwrap(), effects);
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
        assert_eq!(value["declared_fee"], serde_json::json!({"kind": "none"}));
    }

    #[test]
    fn a_value_claim_must_say_what_it_moves() {
        // The SDK forces a choice but cannot check it: moves_nothing() emits
        // the same empty effects that safe and polymarket ship today. What it
        // refuses is a value claim that then declares nothing.
        let empty = ClaimEffects::moves_value();
        assert!(
            empty
                .validate()
                .unwrap_err()
                .contains("a non-zero debit or a destination")
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
        // A debit with its destination says it; so does a spender alone.
        assert!(send().validate().is_ok());
        assert!(
            ClaimEffects::moves_value()
                .destination("base", "0x01")
                .validate()
                .is_ok()
        );
        assert!(ClaimEffects::moves_nothing().validate().is_ok());
    }

    #[test]
    fn zero_amounts_do_not_count_as_declaring_value() {
        // A "0" debit would bring back the empty claim under another name.
        let zero_debit = ClaimEffects::moves_value()
            .debit(&ClaimAsset::new("base", "usdc"), "0")
            .destination("base", "0x01");
        assert!(zero_debit.validate().unwrap_err().contains("is zero"));
        // A "0" fee is honest for a fee-bearing class whose gas someone else
        // pays, but it is not an effect on its own.
        let zero_fee = ClaimEffects::moves_value().fee(&ClaimAsset::new("base", "native"), "0");
        assert!(zero_fee.validate().unwrap_err().contains("non-zero debit"));
        assert!(
            ClaimEffects::moves_value()
                .debit(&ClaimAsset::new("base", "usdc"), "1")
                .destination("base", "0x01")
                .fee(&ClaimAsset::new("base", "native"), "0")
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn a_debit_must_say_where_the_value_goes() {
        // Broker checks only the destinations a claim names, so a debit with
        // none would pass the wallet's destination policy unchecked.
        let unnamed =
            ClaimEffects::moves_value().debit(&ClaimAsset::new("base", "usdc"), "1000000");
        assert!(
            unnamed
                .validate()
                .unwrap_err()
                .contains("allowed_destinations")
        );
        // Saying so explicitly is allowed, and greppable.
        assert!(
            unnamed
                .clone()
                .debits_without_destination()
                .validate()
                .is_ok()
        );
        // The opt-out is not a way to declare nothing.
        assert!(
            ClaimEffects::moves_value()
                .destination("base", "0x01")
                .debits_without_destination()
                .validate()
                .is_err()
        );
        assert!(
            ClaimEffects::moves_nothing()
                .debits_without_destination()
                .validate()
                .is_err()
        );
        // A debit settling on another chain still names its destination.
        assert!(
            ClaimEffects::moves_value()
                .debit(&ClaimAsset::new("hyperliquid", "usdc"), "5000000")
                .destination("arbitrum", "0xAbC0000000000000000000000000000000000001")
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn an_authorization_declares_its_spender_as_a_destination() {
        // A token allowance or operator approval moves nothing when signed,
        // but lets the spender move value later, so the policy must see it.
        let approval = ClaimEffects::moves_value()
            .destination("polygon", "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E");
        assert!(approval.validate().is_ok());
        let jcs = petal_use_claim(
            &ctx(),
            "token.approve",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(3)],
            &approval,
            ClaimNonce::Derived,
        )
        .unwrap()
        .into_jcs();
        assert_eq!(ClaimEffects::from_claim_jcs(&jcs).unwrap(), approval);
    }

    #[test]
    fn a_claim_carries_at_most_one_fee() {
        let twice = ClaimEffects::moves_value()
            .debit(&ClaimAsset::new("hyperliquid", "usdc"), "100")
            .destination("hyperliquid", "0x01")
            .fee(&ClaimAsset::new("hyperliquid", "usdc"), "100")
            .fee(&ClaimAsset::new("arbitrum", "native"), "1");
        assert!(twice.validate().unwrap_err().contains("at most one fee"));
    }

    #[test]
    fn declared_totals_per_asset_fit_in_256_bits() {
        let max = ClaimEffects::moves_value()
            .debit(&ClaimAsset::new("base", "native"), U256_MAX)
            .destination("base", "0x01");
        assert!(max.clone().validate().is_ok());
        let over = max.fee(&ClaimAsset::new("base", "native"), "1");
        assert!(over.validate().unwrap_err().contains("exceeds 2^256-1"));
        // Different assets are separate totals.
        assert!(
            ClaimEffects::moves_value()
                .debit(&ClaimAsset::new("base", "native"), U256_MAX)
                .debit(&ClaimAsset::new("base", "usdc"), U256_MAX)
                .destination("base", "0x01")
                .fee(&ClaimAsset::new("base", "eth"), "1")
                .validate()
                .is_ok()
        );
        assert_eq!(add_decimal("999", "1"), "1000");
        assert_eq!(add_decimal("0", "0"), "0");
        assert_eq!(add_decimal("15", "985"), "1000");
    }

    #[test]
    fn a_claim_declared_as_moving_nothing_cannot_carry_effects() {
        for effects in [
            ClaimEffects::moves_nothing().debit(&ClaimAsset::new("base", "native"), "1"),
            ClaimEffects::moves_nothing().destination("base", "0x01"),
            ClaimEffects::moves_nothing().fee(&ClaimAsset::new("base", "native"), "1"),
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
                .debit(&ClaimAsset::new("base", "native"), "1.5")
                .validate()
                .is_err()
        );
        assert!(
            ClaimEffects::moves_value()
                .fee(&ClaimAsset::new("base", "native"), "1")
                .destination("base", "")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn every_chain_and_asset_a_claim_declares_is_checked() {
        // A valid claim plus one bad value, so each refusal can only come from
        // the check on that value.
        for (effects, expected) in [
            (
                send().debit(&ClaimAsset::native("Base"), "1"),
                r#"chain "Base""#,
            ),
            (
                send().debit(&ClaimAsset::new("base", "usdc "), "1"),
                "debit asset",
            ),
            (send().destination("8453", "0x01"), r#"chain "8453""#),
            (
                send().fee(&ClaimAsset::native("base chain"), "1"),
                r#"chain "base chain""#,
            ),
            (
                send().fee(&ClaimAsset::new("base", " native"), "1"),
                "fee asset",
            ),
        ] {
            let message = effects.validate().expect_err(expected);
            assert!(message.contains(expected), "{expected}: {message}");
        }
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
            .without_route_id()
            .bind::<Confirm>();
        assert_eq!(code(&build(&no_route, "transfer.send", &[payload(1)])), -2);
        let bad_package = RouteCtx::new("transfers/alice/0/confirm.json")
            .package_hash("test-package")
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
        .unwrap()
        .into_jcs();
        assert_eq!(ClaimEffects::from_claim_jcs(&jcs).unwrap(), send());
        assert!(ClaimEffects::from_claim_jcs(b"{}").is_err());
    }

    #[test]
    fn claim_effects_accepts_only_what_the_builder_can_emit() {
        let jcs = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1)],
            &send(),
            ClaimNonce::Derived,
        )
        .unwrap()
        .into_jcs();
        let mut value: serde_json::Value = serde_json::from_slice(&jcs).unwrap();
        let reencode = |value: &serde_json::Value| serde_jcs::to_vec(value).unwrap();

        // Not canonical: the same claim pretty-printed.
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        assert!(
            ClaimEffects::from_claim_jcs(&pretty)
                .unwrap_err()
                .contains("canonical")
        );

        // A field Bloom does not define.
        value["extra"] = serde_json::json!(1);
        assert!(ClaimEffects::from_claim_jcs(&reencode(&value)).is_err());
        value.as_object_mut().unwrap().remove("extra");

        // Effects the builder would refuse: a zero debit.
        value["declared_debits"][0]["amount"] = serde_json::json!("0");
        assert!(
            ClaimEffects::from_claim_jcs(&reencode(&value))
                .unwrap_err()
                .contains("is zero")
        );
        value["declared_debits"][0]["amount"] = serde_json::json!("1000000000000000");

        // A malformed digest.
        value["payload_digest"] = serde_json::json!("ABCD");
        assert!(
            ClaimEffects::from_claim_jcs(&reencode(&value))
                .unwrap_err()
                .contains("payload_digest")
        );
    }

    /// Decode a canonical claim that is valid but for `field`, expecting a
    /// refusal.
    fn refusal(field: &str, bad: &str) -> String {
        let mut value = claim(&send());
        value[field] = serde_json::json!(bad);
        match ClaimEffects::from_claim_jcs(&serde_jcs::to_vec(&value).unwrap()) {
            Ok(_) => panic!("a claim with {field} {bad:?} decoded"),
            Err(message) => message,
        }
    }

    #[test]
    fn claim_effects_decodes_every_suite_the_builder_emits() {
        for suite in [
            CryptoSuite::Secp256k1Keccak256Recoverable,
            CryptoSuite::Secp256k1Sha256Recoverable,
            CryptoSuite::Ed25519Message,
        ] {
            let jcs = petal_use_claim(
                &ctx(),
                "transfer.send",
                suite,
                &[payload(1)],
                &send(),
                ClaimNonce::Derived,
            )
            .unwrap()
            .into_jcs();
            assert_eq!(
                ClaimEffects::from_claim_jcs(&jcs).unwrap(),
                send(),
                "{suite:?}"
            );
        }
    }

    #[test]
    fn claim_effects_refuses_an_origin_the_builder_refuses() {
        // The same refusal, word for word, as ClaimOrigin::new gives.
        let builder = |package: &str, route: &str| {
            format!("claim {}", ClaimOrigin::new(package, route).unwrap_err())
        };
        for route in ["", "R7", "r 7"] {
            let message = refusal("route", route);
            assert!(message.starts_with("claim route id "), "{message}");
            assert_eq!(message, builder(PACKAGE, route));
        }
        let message = refusal("package_hash", "test-package");
        assert!(message.starts_with("claim package hash "), "{message}");
        assert_eq!(message, builder("test-package", "r000007"));
    }

    #[test]
    fn claim_effects_refuses_an_operation_class_the_builder_refuses() {
        // The same refusal, word for word, as ClaimOrigin::claim gives.
        let builder = |class: &str| {
            let refused = ClaimOrigin::new(PACKAGE, "r000007")
                .unwrap()
                .claim(
                    class,
                    CryptoSuite::Ed25519Message,
                    &[payload(1)],
                    &ClaimEffects::moves_nothing(),
                    ClaimNonce::Derived,
                )
                .unwrap_err();
            match refused {
                DispatchResponse::Error { code: -3, message } => format!("claim {message}"),
                other => panic!("expected a -3 refusal, got {other:?}"),
            }
        };
        for class in ["BAD CLASS", "Transfer.Send", ""] {
            let message = refusal("operation_class", class);
            assert!(message.starts_with("claim operation class "), "{message}");
            assert_eq!(message, builder(class));
        }
    }

    #[test]
    fn claim_effects_refuses_an_unsupported_crypto_suite() {
        for suite in [
            "not-a-suite",
            "SECP256K1-KECCAK256-RECOVERABLE",
            "ed25519-message ",
            "",
        ] {
            assert_eq!(
                refusal("crypto_suite", suite),
                format!("claim crypto_suite {suite:?} is not a supported suite")
            );
        }
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

    #[test]
    fn the_sign_request_carries_the_claim_s_class_suite_and_payloads() {
        let claim = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Secp256k1Keccak256Recoverable,
            &[payload(1)],
            &send(),
            ClaimNonce::Derived,
        )
        .unwrap();
        let request = claim.sign_request("alice", SignSelector::Exact).unwrap();
        assert_eq!(request.operation_class, "transfer.send");
        assert_eq!(
            request.signature_algorithm,
            "secp256k1-keccak256-recoverable"
        );
        assert_eq!(request.preimage, payload(1).preimage);
        assert_eq!(request.claimed_hash, payload(1).claimed_hash);
        assert_eq!(request.petal_use_claim_jcs, claim.jcs());

        let batch = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Ed25519Message,
            &[payload(1), payload(2)],
            &ClaimEffects::moves_nothing(),
            ClaimNonce::Derived,
        )
        .unwrap();
        // One payload request cannot stand for a batch claim.
        assert_eq!(
            code(
                &batch
                    .sign_request("alice", SignSelector::Exact)
                    .unwrap_err()
            ),
            -3
        );
        let request = batch.batch_request("alice", SignSelector::Reusable);
        assert_eq!(request.payloads, vec![payload(1), payload(2)]);
        assert_eq!(request.operation_class, "transfer.send");
        assert_eq!(request.signature_algorithm, "ed25519-message");
        assert_eq!(request.petal_use_claim_jcs, batch.jcs());
    }

    #[test]
    fn payloads_bloom_would_refuse_are_refused_here() {
        let build = |payloads: &[PayloadSignItem]| {
            petal_use_claim(
                &ctx(),
                "transfer.send",
                CryptoSuite::Ed25519Message,
                payloads,
                &ClaimEffects::moves_nothing(),
                ClaimNonce::Derived,
            )
            .map(|_| ())
            .map_err(|err| code(&err))
        };
        let empty = PayloadSignItem {
            preimage: Vec::new(),
            claimed_hash: [1; 32],
        };
        assert_eq!(build(std::slice::from_ref(&empty)), Err(-3));
        assert_eq!(build(&[payload(1), empty]), Err(-3));
        assert_eq!(build(&vec![payload(1); 32]), Ok(()));
        assert_eq!(build(&vec![payload(1); 33]), Err(-3));
        let child = |len: usize| PayloadSignItem {
            preimage: vec![1; len],
            claimed_hash: [1; 32],
        };
        let max = crate::MAX_SIGN_BATCH_CHILD_BYTES;
        assert_eq!(build(&[child(max), payload(1)]), Ok(()));
        assert_eq!(build(&[child(max + 1), payload(1)]), Err(-3));
        // Eight full children reach the 512 KiB total exactly; one more byte
        // is over.
        assert_eq!(build(&vec![child(max); 8]), Ok(()));
        let mut over = vec![child(max); 8];
        over.push(payload(1));
        assert_eq!(build(&over), Err(-3));
        // A single payload has no batch size limit in Bloom, only non-empty.
        assert_eq!(build(&[child(max + 1)]), Ok(()));
    }

    #[test]
    fn a_claim_origin_can_be_built_without_a_route_context() {
        let origin = ClaimOrigin::new(PACKAGE, "r000007").unwrap();
        let from_ctx = ClaimOrigin::from_ctx(&ctx()).unwrap();
        assert_eq!(origin, from_ctx);
        let claim = origin
            .claim(
                "transfer.send",
                CryptoSuite::Ed25519Message,
                &[payload(1)],
                &ClaimEffects::moves_nothing(),
                ClaimNonce::Derived,
            )
            .unwrap();
        let same = petal_use_claim(
            &ctx(),
            "transfer.send",
            CryptoSuite::Ed25519Message,
            &[payload(1)],
            &ClaimEffects::moves_nothing(),
            ClaimNonce::Derived,
        )
        .unwrap();
        assert_eq!(claim, same);
        for (package, route) in [
            ("test-package", "r000007"),
            (&PACKAGE.to_uppercase()[..], "r000007"),
            (PACKAGE, ""),
            (PACKAGE, "R7"),
        ] {
            assert!(
                ClaimOrigin::new(package, route).is_err(),
                "{package} {route}"
            );
        }
    }

    #[test]
    fn the_default_test_context_can_build_a_claim() {
        let ctx = RouteCtx::new("transfers/alice/0/confirm.json")
            .account("alice", 0)
            .bind::<Confirm>();
        let origin = ClaimOrigin::from_ctx(&ctx).unwrap();
        assert_eq!(origin.package_hash(), crate::testing::PACKAGE_HASH);
        assert_eq!(origin.route(), crate::testing::ROUTE_ID);
    }

    #[test]
    fn assets_and_destinations_are_printable_ascii() {
        let with_destination = |destination: &str| {
            ClaimEffects::moves_value()
                .destination("base", destination)
                .validate()
        };
        assert!(with_destination("0xAbC0000000000000000000000000000000000001").is_ok());
        assert!(with_destination("7EcDhSYGxXyscszYEp35KHN8vvw3svAuLKTzXwCFLtV").is_ok());
        for bad in [
            " 0x01",
            "0x01 ",
            "0x\u{200b}01",
            "0x\u{202e}01",
            "0x\u{a0}01",
            "0x\u{feff}01",
            "0x\u{2028}01",
            "\u{e9}",
        ] {
            let message = with_destination(bad).unwrap_err();
            assert!(message.contains("printable ASCII"), "{bad:?}: {message}");
        }
        assert!(with_destination(&"a".repeat(256)).is_ok());
        assert!(with_destination(&"a".repeat(257)).is_err());
    }
}
