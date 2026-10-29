//! Version 1 prepared-signing wire contract. All byte vectors are JSON arrays.
//! Hosts must parse and validate independently; declarations are not authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sha3::Keccak256;

pub const VERSION: u32 = 1;
pub const MAX_PREIMAGE: usize = 64 * 1024;
pub const MAX_REVIEW: usize = 64 * 1024;
pub const MAX_REQUEST: usize = 1024 * 1024;
const MAX_U256: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub wallet: String,
    pub operation_key: String,
    pub action: PreparedAction,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {
    pub version: u32,
    pub wallet: String,
    pub operation_key: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedAction {
    pub operation_class: String,
    pub payload: SigningPayload,
    pub review: ReviewArtifact,
    pub spend: Spend,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningPayload {
    pub suite: CryptoSuite,
    pub preimage: Vec<u8>,
    pub hash: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CryptoSuite {
    #[serde(rename = "secp256k1-keccak256-recoverable")]
    Keccak256,
    #[serde(rename = "secp256k1-sha256-recoverable")]
    Sha256,
    #[serde(rename = "ed25519-message")]
    Ed25519Message,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewArtifact {
    pub format: String,
    pub version: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spend {
    Unspecified,
    None,
    Bounded { debits: Vec<Debit> },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Debit {
    pub network: String,
    pub asset: Asset,
    pub amount: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Asset {
    Erc20 { address: String },
    Erc1155 { address: String, token_id: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum SigningOutcome {
    Signed { signature: Vec<u8> },
    ApprovalPending { action_id: String, expires_ms: u64 },
    RecoveryRequired { status: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningError {
    pub code: ErrorCode,
    pub message: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Invalid,
    Unsupported,
    Conflict,
    Denied,
    Cancelled,
    Expired,
    Backend,
}

fn invalid(message: &str) -> SigningError {
    SigningError {
        code: ErrorCode::Invalid,
        message: message.into(),
    }
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn uint256(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
        && (value.len() < MAX_U256.len() || value.len() == MAX_U256.len() && value <= MAX_U256)
}
fn address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn network(value: &str) -> bool {
    value
        .strip_prefix("evm-")
        .is_some_and(|s| uint256(s) && s != "0")
}
impl SigningPayload {
    fn new(suite: CryptoSuite, preimage: Vec<u8>) -> Result<Self, SigningError> {
        if preimage.len() > MAX_PREIMAGE {
            return Err(invalid("preimage exceeds limit"));
        }
        let hash = Self::hash_for(suite, &preimage);
        Ok(Self {
            suite,
            preimage,
            hash,
        })
    }
    pub fn keccak256(preimage: Vec<u8>) -> Result<Self, SigningError> {
        Self::new(CryptoSuite::Keccak256, preimage)
    }
    pub fn sha256(preimage: Vec<u8>) -> Result<Self, SigningError> {
        Self::new(CryptoSuite::Sha256, preimage)
    }
    pub fn ed25519_message(preimage: Vec<u8>) -> Result<Self, SigningError> {
        Self::new(CryptoSuite::Ed25519Message, preimage)
    }
    pub fn hash_for(suite: CryptoSuite, preimage: &[u8]) -> [u8; 32] {
        match suite {
            CryptoSuite::Keccak256 => Keccak256::digest(preimage).into(),
            CryptoSuite::Sha256 | CryptoSuite::Ed25519Message => Sha256::digest(preimage).into(),
        }
    }
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.preimage.len() > MAX_PREIMAGE
            || self.hash != Self::hash_for(self.suite, &self.preimage)
        {
            return Err(invalid("invalid payload or hash"));
        }
        Ok(())
    }
}
impl ReviewArtifact {
    pub fn new(
        format: impl Into<String>,
        version: u32,
        bytes: Vec<u8>,
    ) -> Result<Self, SigningError> {
        let r = Self {
            format: format.into(),
            version,
            bytes,
        };
        r.validate()?;
        Ok(r)
    }
    pub fn validate(&self) -> Result<(), SigningError> {
        if !text(&self.format, 128) || self.version == 0 || self.bytes.len() > MAX_REVIEW {
            return Err(invalid("invalid review artifact"));
        }
        Ok(())
    }
}
impl Spend {
    pub fn erc20(
        network: impl Into<String>,
        address: impl Into<String>,
        amount: impl Into<String>,
    ) -> Result<Self, SigningError> {
        let s = Self::Bounded {
            debits: vec![Debit {
                network: network.into(),
                asset: Asset::Erc20 {
                    address: address.into(),
                },
                amount: amount.into(),
            }],
        };
        s.validate()?;
        Ok(s)
    }
    pub fn erc1155(
        network: impl Into<String>,
        address: impl Into<String>,
        token_id: impl Into<String>,
        amount: impl Into<String>,
    ) -> Result<Self, SigningError> {
        let s = Self::Bounded {
            debits: vec![Debit {
                network: network.into(),
                asset: Asset::Erc1155 {
                    address: address.into(),
                    token_id: token_id.into(),
                },
                amount: amount.into(),
            }],
        };
        s.validate()?;
        Ok(s)
    }
    pub fn validate(&self) -> Result<(), SigningError> {
        if let Self::Bounded { debits } = self {
            if debits.is_empty() || debits.len() > 32 {
                return Err(invalid("invalid debit count"));
            }
            for (index, d) in debits.iter().enumerate() {
                if debits[..index]
                    .iter()
                    .any(|previous| previous.network == d.network && previous.asset == d.asset)
                {
                    return Err(invalid("duplicate debit asset"));
                }
                if !network(&d.network) || !uint256(&d.amount) {
                    return Err(invalid("invalid debit network or amount"));
                }
                match &d.asset {
                    Asset::Erc20 { address: a } if address(a) => (),
                    Asset::Erc1155 {
                        address: a,
                        token_id,
                    } if address(a) && uint256(token_id) => (),
                    _ => return Err(invalid("invalid debit asset")),
                }
            }
        }
        Ok(())
    }
}
impl PreparedAction {
    pub fn new(
        operation_class: impl Into<String>,
        payload: SigningPayload,
        review: ReviewArtifact,
    ) -> Result<Self, SigningError> {
        let a = Self {
            operation_class: operation_class.into(),
            payload,
            review,
            spend: Spend::Unspecified,
        };
        a.validate()?;
        Ok(a)
    }
    pub fn with_spend(mut self, spend: Spend) -> Self {
        self.spend = spend;
        self
    }
    pub fn validate(&self) -> Result<(), SigningError> {
        if !text(&self.operation_class, 128) {
            return Err(invalid("invalid operation class"));
        }
        self.payload.validate()?;
        self.review.validate()?;
        self.spend.validate()
    }
}
impl Request {
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.version != VERSION || !text(&self.wallet, 128) || !text(&self.operation_key, 256) {
            return Err(invalid("invalid version, wallet, or operation key"));
        }
        self.action.validate()
    }
}
impl StatusRequest {
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.version != VERSION || !text(&self.wallet, 128) || !text(&self.operation_key, 256) {
            return Err(invalid("invalid status request"));
        }
        Ok(())
    }
}
impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for SigningError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_hash_vectors_and_forged_hash_rejected() {
        let p = SigningPayload::keccak256(b"abc".to_vec()).unwrap();
        assert_eq!(
            hex::encode(p.hash),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
        assert_eq!(
            hex::encode(SigningPayload::sha256(b"abc".to_vec()).unwrap().hash),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut forged = p;
        forged.preimage.push(0);
        assert!(forged.validate().is_err());
        assert!(SigningPayload::sha256(vec![0; MAX_PREIMAGE + 1]).is_err());
    }

    #[test]
    fn raw_uint256_and_canonical_asset_boundaries() {
        let a = "0x0123456789abcdef0123456789abcdef01234567";
        assert!(Spend::erc20("evm-137", a, MAX_U256).is_ok());
        for bad in [
            "",
            "00",
            "01",
            "-1",
            "1.0",
            "115792089237316195423570985008687907853269984665640564039457584007913129639936",
        ] {
            assert!(Spend::erc20("evm-137", a, bad).is_err(), "{bad}");
        }
        assert!(Spend::erc1155("evm-137", a, MAX_U256, "1").is_ok());
        let one = match Spend::erc20("evm-137", a, "1").unwrap() {
            Spend::Bounded { debits } => debits[0].clone(),
            _ => unreachable!(),
        };
        assert!(
            Spend::Bounded {
                debits: vec![one.clone(), one]
            }
            .validate()
            .is_err()
        );
        assert!(Spend::erc20("evm-0137", a, "1").is_err());
        assert!(Spend::erc20("evm-137", a.to_uppercase(), "1").is_err());
    }

    #[test]
    fn second_format_round_trip_preserves_exact_action() {
        let action = PreparedAction::new(
            "example.message",
            SigningPayload::ed25519_message(b"hello".to_vec()).unwrap(),
            ReviewArtifact::new("example.message", 2, b"review".to_vec()).unwrap(),
        )
        .unwrap();
        let req = Request {
            version: VERSION,
            wallet: "wallet".into(),
            operation_key: "message-1".into(),
            action,
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: Request = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, req);
        parsed.validate().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["action"]["payload"]["hash"][0] = 255.into();
        assert!(
            serde_json::from_value::<Request>(value)
                .unwrap()
                .validate()
                .is_err()
        );
        assert!(ReviewArtifact::new("example", 1, vec![0; MAX_REVIEW + 1]).is_err());
    }
}
