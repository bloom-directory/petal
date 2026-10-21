//! Supported generic message encodings. Protocol meaning and independent
//! verification remain with the Petal and Broker respectively.
pub use alloy_dyn_abi::eip712::TypedData;
use alloy_primitives::{eip191_hash_message, keccak256};

use crate::{
    ClaimEffects, ClaimNonce, ClaimOrigin, CryptoSuite, DispatchResponse, PayloadSignItem,
    PayloadSignRequest, SignSelector, error,
};

/// Exact payload, signing hash and complete public review built together.
#[derive(Clone, Debug)]
pub struct EvmMessage {
    payload: PayloadSignItem,
    review: Vec<u8>,
}

impl EvmMessage {
    /// EIP-191 personal signing, using the UTF-8 byte length, not character count.
    pub fn personal(message: &str) -> Result<Self, DispatchResponse> {
        if message.len() > 8192 {
            return Err(error(
                -3,
                "personal message exceeds the supported 8192-byte profile",
            ));
        }
        let mut preimage = format!("\x19Ethereum Signed Message:\n{}", message.len()).into_bytes();
        preimage.extend_from_slice(message.as_bytes());
        Self::new(
            preimage,
            eip191_hash_message(message).into(),
            serde_json::json!({"schema":"bloom.message.review.v1","kind":"personal","message":message}),
        )
    }

    /// Generic EIP-712. This describes typed fields, not venue-specific economic
    /// meaning. Use the Petal's protocol adapter for specialized Broker profiles.
    pub fn eip712(typed: &TypedData) -> Result<Self, DispatchResponse> {
        let mut preimage = vec![0x19, 0x01];
        preimage.extend_from_slice(typed.domain.separator().as_slice());
        if typed.primary_type != "EIP712Domain" {
            preimage.extend_from_slice(
                typed
                    .hash_struct()
                    .map_err(|error| crate::error(-3, format!("invalid typed message: {error}")))?
                    .as_slice(),
            );
        } else if typed
            .message
            .as_object()
            .is_none_or(|message| !message.is_empty())
        {
            return Err(error(-3, "domain-only signing cannot carry message fields"));
        }
        let hash = keccak256(&preimage).into();
        Self::new(
            preimage,
            hash,
            serde_json::json!({"schema":"bloom.message.review.v1","kind":"eip712","typed_data":typed}),
        )
    }

    fn new(
        preimage: Vec<u8>,
        claimed_hash: [u8; 32],
        review: serde_json::Value,
    ) -> Result<Self, DispatchResponse> {
        let review = serde_jcs::to_vec(&review)
            .map_err(|error| crate::error(-3, format!("encode message review: {error}")))?;
        Ok(Self {
            payload: PayloadSignItem {
                preimage,
                claimed_hash,
            },
            review,
        })
    }

    /// Build one exact request with explicit effects and retry nonce. The caller
    /// cannot mutate completed payload/hash/review fields through this API.
    pub fn prepare(
        &self,
        origin: &ClaimOrigin,
        wallet: &str,
        operation_class: &str,
        effects: &ClaimEffects,
        nonce: ClaimNonce,
    ) -> Result<ReviewedSignRequest, DispatchResponse> {
        crate::validate_wallet_id(wallet).map_err(|message| error(-3, message))?;
        let claim = origin.claim(
            operation_class,
            CryptoSuite::Secp256k1Keccak256Recoverable,
            std::slice::from_ref(&self.payload),
            effects,
            nonce,
        )?;
        Ok(ReviewedSignRequest {
            request: claim.reviewed_sign_request(
                wallet,
                SignSelector::Exact,
                self.review.clone(),
                None,
            )?,
        })
    }
}

/// A completed supported request. Retry hints and advisory data may be attached;
/// use the existing low-level request API for manual or protocol-specific work.
#[derive(Clone, Debug)]
pub struct ReviewedSignRequest {
    request: PayloadSignRequest,
}

impl ReviewedSignRequest {
    pub fn request(&self) -> &PayloadSignRequest {
        &self.request
    }
    pub fn with_approval_hint(mut self, hint: Option<String>) -> Self {
        self.request.approval_hint = hint;
        self
    }
    pub fn with_advisory(mut self, advisory: Option<Vec<u8>>) -> Self {
        self.request.advisory = advisory;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;

    fn origin() -> ClaimOrigin {
        ClaimOrigin::new(&"ab".repeat(32), "r000001").unwrap()
    }

    fn mail() -> TypedData {
        serde_json::from_value(serde_json::json!({
            "types": {
                "EIP712Domain": [{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],
                "Person": [{"name":"name","type":"string"},{"name":"wallet","type":"address"}],
                "Mail": [{"name":"from","type":"Person"},{"name":"to","type":"Person"},{"name":"contents","type":"string"}]
            },
            "primaryType":"Mail",
            "domain":{"name":"Ether Mail","version":"1","chainId":1,"verifyingContract":"0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"},
            "message":{"from":{"name":"Cow","wallet":"0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"},"to":{"name":"Bob","wallet":"0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB"},"contents":"Hello, Bob!"}
        })).unwrap()
    }

    #[test]
    fn published_eip712_mail_hash_and_review_are_constructed_together() {
        let prepared = EvmMessage::eip712(&mail())
            .unwrap()
            .prepare(
                &origin(),
                "alice",
                "example.message",
                &ClaimEffects::moves_nothing(),
                ClaimNonce::Explicit([7; 16]),
            )
            .unwrap();
        let request = prepared.request();
        assert_eq!(
            B256::from(request.claimed_hash).to_string(),
            "0xbe609aee343fb3c4b28e1df9e632fca64fcfaede20f02e86244efddf30957bd2"
        );
        assert_eq!(&request.preimage[..2], &[0x19, 0x01]);
        assert_eq!(request.preimage.len(), 66);
        let review: serde_json::Value =
            serde_json::from_slice(request.action.as_ref().unwrap()).unwrap();
        assert_eq!(review["kind"], "eip712");
        assert_eq!(review["typed_data"]["message"]["contents"], "Hello, Bob!");
        let claim: serde_json::Value =
            serde_json::from_slice(&request.petal_use_claim_jcs).unwrap();
        assert_eq!(claim["nonce"], "07070707070707070707070707070707");
        assert_eq!(claim["operation_class"], request.operation_class);
    }

    #[test]
    fn personal_unicode_uses_byte_length_and_preserves_retry_identity() {
        let message = EvmMessage::personal("é").unwrap();
        let prepared = message
            .prepare(
                &origin(),
                "alice",
                "example.message",
                &ClaimEffects::moves_nothing(),
                ClaimNonce::Derived,
            )
            .unwrap();
        assert_eq!(
            prepared.request().preimage,
            b"\x19Ethereum Signed Message:\n2\xc3\xa9"
        );
        let retry = prepared.clone().with_approval_hint(Some("approval".into()));
        assert_eq!(retry.request().preimage, prepared.request().preimage);
        assert_eq!(
            retry.request().claimed_hash,
            prepared.request().claimed_hash
        );
        assert_eq!(retry.request().action, prepared.request().action);
        assert_eq!(
            retry.request().petal_use_claim_jcs,
            prepared.request().petal_use_claim_jcs
        );
        assert!(
            message
                .prepare(
                    &origin(),
                    "../other",
                    "example.message",
                    &ClaimEffects::moves_nothing(),
                    ClaimNonce::Derived
                )
                .is_err()
        );
        assert!(EvmMessage::personal(&"x".repeat(8193)).is_err());
    }

    #[test]
    fn domain_only_typed_data_cannot_smuggle_message_fields() {
        let mut typed = mail();
        typed.primary_type = "EIP712Domain".into();
        assert!(EvmMessage::eip712(&typed).is_err());
    }
}
