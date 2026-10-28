//! Typed accounting declarations for payload signing.
//!
//! These helpers bind declarations to the exact package, route and ordered
//! payloads. They do not prove economic effects or grant signing authority:
//! Broker's supported protocol verifier and reviewed approval remain necessary.
use crate::{Ctx, PayloadSignItem, SdkError};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub chain: String,
    pub asset: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Debit {
    pub asset: Asset,
    /// Canonical unsigned integer base units, never a price or human decimal.
    pub amount: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub chain: String,
    pub destination: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fee {
    None,
    Fee {
        chain: String,
        asset: String,
        amount: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionEffect {
    None,
    RequiresSeparateApproval,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effects {
    network: String,
    permission: PermissionEffect,
    debits: Vec<Debit>,
    destinations: Vec<Destination>,
    fee: Fee,
}
impl Effects {
    /// Explicitly classify continuing authority such as approvals, credential
    /// creation or delegation as RequiresSeparateApproval, even with no debit.
    pub fn new(network: impl Into<String>, permission: PermissionEffect) -> Self {
        Self {
            network: network.into(),
            permission,
            debits: vec![],
            destinations: vec![],
            fee: Fee::None,
        }
    }
    pub fn with_debits(mut self, debits: Vec<Debit>) -> Self {
        self.debits = debits;
        self
    }
    pub fn with_destinations(mut self, destinations: Vec<Destination>) -> Self {
        self.destinations = destinations;
        self
    }
    pub fn with_fee(mut self, fee: Fee) -> Self {
        self.fee = fee;
        self
    }

    /// Preserve the existing effects-v1 wire contract. Machine fills the exact
    /// spending KeyRef from its authenticated account selection; Petals cannot
    /// supply their own account identities through this builder.
    pub fn batch_claim(
        &self,
        ctx: &Ctx,
        operation_class: &str,
        crypto_suite: &str,
        payloads: &[PayloadSignItem],
    ) -> Result<Vec<u8>, SdkError> {
        let route = ctx
            .params
            .iter()
            .find_map(|(k, v)| (k == "bloom.route_id").then_some(v.as_str()))
            .ok_or_else(|| invalid("trusted Petal route id is unavailable"))?;
        if !token(&self.network)
            || !token(operation_class)
            || !token(crypto_suite)
            || !token(route)
            || !hex_digest(&ctx.package_hash)
            || payloads.len() > 128
            || payloads.iter().any(|p| p.preimage.is_empty())
        {
            return Err(invalid("invalid effects signing context"));
        }
        for (i, debit) in self.debits.iter().enumerate() {
            if debit.asset.chain != self.network
                || debit.asset.asset.is_empty()
                || !quantity(&debit.amount)
                || self.debits[..i].iter().any(|d| d.asset == debit.asset)
            {
                return Err(invalid(
                    "debits need unique network-bound assets and canonical uint256 quantities",
                ));
            }
        }
        if self
            .destinations
            .iter()
            .any(|d| !token(&d.chain) || d.destination.is_empty())
        {
            return Err(invalid("invalid declared destination"));
        }
        if let Fee::Fee {
            chain,
            asset,
            amount,
        } = &self.fee
        {
            if chain != &self.network || asset.is_empty() || !quantity(amount) {
                return Err(invalid("invalid network-bound fee"));
            }
        }
        let payload_digest = crate::payload_batch_digest(payloads)?;
        let nonce = Sha256::digest(
            [
                ctx.package_hash.as_bytes(),
                route.as_bytes(),
                operation_class.as_bytes(),
                payload_digest.as_slice(),
            ]
            .concat(),
        );
        serde_jcs::to_vec(&serde_json::json!({
            "package_hash":ctx.package_hash,"route":route,"operation_class":operation_class,
            "crypto_suite":crypto_suite,"payload_digest":hex(&payload_digest),
            "ordered_hashes":payloads.iter().map(|p|hex(&p.claimed_hash)).collect::<Vec<_>>(),
            "declared_debits":self.debits,"declared_destinations":self.destinations,"declared_fee":self.fee,
            "economic_effects":{"schema_version":1,"permission_effect":self.permission,
                "payload_contexts":(0..payloads.len()).map(|i|serde_json::json!({"payload_index":i,"network":self.network,"spending_accounts":[]})).collect::<Vec<_>>()},
            "nonce":hex(&nonce[..16]),"claim_assurance":{"kind":"machine_asserted"}
        })).map_err(|_|invalid("encode economic effects claim"))
    }
}
fn invalid(message: &str) -> SdkError {
    SdkError::Message(message.into())
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn quantity(value: &str) -> bool {
    const MAX: &str =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
        && (value.len() < MAX.len() || (value.len() == MAX.len() && value <= MAX))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawCtx, RouteIdentity};
    struct Route;
    impl RouteIdentity for Route {
        const PATH: &'static str = "trade/[wallet]/submit";
        const CANONICAL_PATH: &'static str = "trade/[wallet]/submit";
        const PARAMS: &'static [(&'static str, usize)] = &[];
    }
    fn context() -> Ctx {
        Ctx::bind::<Route>(RawCtx {
            petal_root: "example".into(),
            package_hash: "ab".repeat(32),
            path: "trade/alice/submit".into(),
            params: vec![("bloom.route_id".into(), "r000001".into())],
            actor: None,
        })
    }
    fn payloads() -> Vec<PayloadSignItem> {
        vec![
            PayloadSignItem {
                preimage: vec![1, 2, 3],
                claimed_hash: [1; 32],
            },
            PayloadSignItem {
                preimage: vec![4],
                claimed_hash: [2; 32],
            },
        ]
    }
    #[test]
    fn claims_bind_package_route_class_payload_order_and_network_without_fabricated_accounts() {
        let effects = Effects::new("evm-137", PermissionEffect::None).with_debits(vec![Debit {
            asset: Asset {
                chain: "evm-137".into(),
                asset: "token".into(),
            },
            amount: "123".into(),
        }]);
        let make = |ctx: &Ctx, p: &[PayloadSignItem], class: &str| {
            effects
                .batch_claim(ctx, class, "secp256k1-keccak256-recoverable", p)
                .unwrap()
        };
        let original = make(&context(), &payloads(), "example.buy");
        let v: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(
            v["economic_effects"]["payload_contexts"][1]["payload_index"],
            1
        );
        assert_eq!(
            v["economic_effects"]["payload_contexts"][0]["spending_accounts"],
            serde_json::json!([])
        );
        assert_eq!(v["declared_debits"][0]["amount"], "123");
        let mut changed = context();
        changed.package_hash = "cd".repeat(32);
        assert_ne!(original, make(&changed, &payloads(), "example.buy"));
        changed = context();
        changed.params[0].1 = "r000002".into();
        assert_ne!(original, make(&changed, &payloads(), "example.buy"));
        let mut reversed = payloads();
        reversed.reverse();
        assert_ne!(original, make(&context(), &reversed, "example.buy"));
        assert_ne!(original, make(&context(), &payloads(), "example.sell"));
        assert_eq!(original, make(&context(), &payloads(), "example.buy"));
    }
    #[test]
    fn malformed_effects_and_duplicate_debits_fail_closed() {
        let debit = Debit {
            asset: Asset {
                chain: "evm-1".into(),
                asset: "token".into(),
            },
            amount: "1".into(),
        };
        for amount in [
            "",
            "01",
            "-1",
            "0.1",
            "1e6",
            "115792089237316195423570985008687907853269984665640564039457584007913129639936",
        ] {
            let mut d = debit.clone();
            d.amount = amount.into();
            assert!(
                Effects::new("evm-1", PermissionEffect::None)
                    .with_debits(vec![d])
                    .batch_claim(&context(), "test", "ed25519-message", &payloads())
                    .is_err()
            );
        }
        for debits in [
            vec![debit.clone(), debit.clone()],
            vec![Debit {
                asset: Asset {
                    chain: "evm-2".into(),
                    asset: "token".into(),
                },
                ..debit
            }],
        ] {
            assert!(
                Effects::new("evm-1", PermissionEffect::None)
                    .with_debits(debits)
                    .batch_claim(&context(), "test", "ed25519-message", &payloads())
                    .is_err()
            );
        }
        assert!(
            Effects::new("evm-1", PermissionEffect::None)
                .batch_claim(&context(), "test", "ed25519-message", &[])
                .is_err()
        );
        let mut ctx = context();
        ctx.params.clear();
        assert!(
            Effects::new("evm-1", PermissionEffect::None)
                .batch_claim(&ctx, "test", "ed25519-message", &payloads())
                .is_err()
        );
    }
    #[test]
    fn continuing_permissions_are_explicit_even_without_debits() {
        let bytes = Effects::new("evm-1", PermissionEffect::RequiresSeparateApproval)
            .batch_claim(&context(), "test", "ed25519-message", &payloads())
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["economic_effects"]["permission_effect"],
            "requires_separate_approval"
        );
    }
}
