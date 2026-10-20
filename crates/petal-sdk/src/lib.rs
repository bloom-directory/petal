//! Foundational framework and Bloom host SDK for route petals.
#![allow(
    clippy::crate_in_macro_def,
    clippy::items_after_test_module,
    clippy::needless_lifetimes,
    clippy::too_many_arguments
)]

#[allow(clippy::all)]
pub mod bindings {
    include!("route_file.rs");
}

fn component_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    let bytes = sdk::random_bytes(buf.len()).map_err(|_| getrandom::Error::UNSUPPORTED)?;
    buf.copy_from_slice(&bytes);
    Ok(())
}

getrandom::register_custom_getrandom!(component_getrandom);

mod claim;
pub use claim::{
    ClaimAsset, ClaimDebit, ClaimDestination, ClaimEffects, ClaimFee, ClaimNonce, ClaimOrigin,
    CryptoSuite, PetalClaim, petal_use_claim,
};

pub use bindings::bloom::route::types::EntryKind;
pub use bindings::{Ctx as RawCtx, Entry, Guest as RawGuest, RouteError, RouteMeta};

pub trait RouteIdentity {
    const PATH: &'static str;
    const CANONICAL_PATH: &'static str;
    const PARAMS: &'static [(&'static str, usize)];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ctx {
    pub petal_root: String,
    pub package_hash: String,
    pub path: String,
    pub params: Vec<(String, String)>,
    pub actor: Option<String>,
    identity_path: &'static str,
    identity_canonical_path: &'static str,
    identity_params: &'static [(&'static str, usize)],
}

impl Ctx {
    pub fn bind<I: RouteIdentity>(raw: RawCtx) -> Self {
        Self {
            petal_root: raw.petal_root,
            package_hash: raw.package_hash,
            path: raw.path,
            params: raw.params,
            actor: raw.actor,
            identity_path: I::PATH,
            identity_canonical_path: I::CANONICAL_PATH,
            identity_params: I::PARAMS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchResponse {
    Read(Vec<u8>),
    Write,
    Error { code: i32, message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignSelector {
    Exact,
    Reusable,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PetalKeyRequest {
    pub wallet_id: String,
    pub key_slot: String,
    pub allowed_routes: Vec<String>,
    pub allowed_operation_classes: Vec<String>,
    pub allowed_crypto_suites: Vec<String>,
    pub maximum_lifetime_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PetalKeyOutcome {
    Pending {
        operation_id: String,
        scope_digest: String,
    },
    Ready {
        operation_id: String,
        scope_digest: String,
        key_ref_jcs: Vec<u8>,
        addresses: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadSignRequest {
    pub wallet: String,
    pub preimage: Vec<u8>,
    pub claimed_hash: [u8; 32],
    pub signature_algorithm: String,
    pub operation_class: String,
    pub petal_use_claim_jcs: Vec<u8>,
    pub claim_assurance_evidence: Option<Vec<u8>>,
    pub approval_hint: Option<String>,
    pub action: Option<Vec<u8>>,
    pub advisory: Option<Vec<u8>>,
    pub selector: SignSelector,
    pub key_ref_jcs: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadSignItem {
    pub preimage: Vec<u8>,
    pub claimed_hash: [u8; 32],
}

/// One atomic signing operation over an immutable ordered payload list.
///
/// The wallet, suite, operation class, approval, selector, and optional
/// Signer-owned key reference apply to the complete batch. They deliberately
/// cannot vary between children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadBatchSignRequest {
    pub wallet: String,
    pub payloads: Vec<PayloadSignItem>,
    pub signature_algorithm: String,
    pub operation_class: String,
    pub petal_use_claim_jcs: Vec<u8>,
    pub claim_assurance_evidence: Option<Vec<u8>>,
    pub approval_hint: Option<String>,
    pub action: Option<Vec<u8>>,
    pub advisory: Option<Vec<u8>>,
    pub selector: SignSelector,
    pub key_ref_jcs: Option<Vec<u8>>,
}

pub const PAYLOAD_BATCH_DIGEST_DOMAIN_V1: &[u8] = b"bloom.petal.payload-batch.v1\0";

/// Bloom signs at most this many payloads in one batch.
pub const MAX_SIGN_BATCH_ITEMS: usize = 32;
/// Bloom refuses a batch child larger than this.
pub const MAX_SIGN_BATCH_CHILD_BYTES: usize = 64 * 1024;
/// Bloom refuses a batch whose preimages together exceed this.
pub const MAX_SIGN_BATCH_PAYLOAD_BYTES: usize = 512 * 1024;

/// Check a payload batch against the limits Bloom enforces before signing:
/// 1 to [`MAX_SIGN_BATCH_ITEMS`] items, none empty, none larger than
/// [`MAX_SIGN_BATCH_CHILD_BYTES`], and [`MAX_SIGN_BATCH_PAYLOAD_BYTES`] in all.
pub fn validate_payload_batch(payloads: &[PayloadSignItem]) -> Result<(), SdkError> {
    if payloads.is_empty() || payloads.len() > MAX_SIGN_BATCH_ITEMS {
        return Err(SdkError::Message(format!(
            "payload signing batch must contain 1..={MAX_SIGN_BATCH_ITEMS} items"
        )));
    }
    let mut total = 0usize;
    for (index, payload) in payloads.iter().enumerate() {
        if payload.preimage.is_empty() {
            return Err(SdkError::Message(format!(
                "payload item {index} has an empty preimage"
            )));
        }
        if payload.preimage.len() > MAX_SIGN_BATCH_CHILD_BYTES {
            return Err(SdkError::Message(format!(
                "payload item {index} exceeds {MAX_SIGN_BATCH_CHILD_BYTES} bytes"
            )));
        }
        total += payload.preimage.len();
    }
    if total > MAX_SIGN_BATCH_PAYLOAD_BYTES {
        return Err(SdkError::Message(format!(
            "payload batch exceeds {MAX_SIGN_BATCH_PAYLOAD_BYTES} bytes"
        )));
    }
    Ok(())
}

/// The digest a Petal use claim binds, for one payload or a batch. Bloom
/// refuses an empty preimage on both paths, so this does too.
pub fn payload_batch_digest(payloads: &[PayloadSignItem]) -> Result<[u8; 32], SdkError> {
    use sha2::{Digest as _, Sha256};

    if payloads.is_empty() {
        return Err(SdkError::Message(
            "payload signing batch must not be empty".into(),
        ));
    }
    if let Some(index) = payloads.iter().position(|item| item.preimage.is_empty()) {
        return Err(SdkError::Message(format!(
            "payload item {index} has an empty preimage"
        )));
    }
    let mut digest = Sha256::new();
    digest.update(PAYLOAD_BATCH_DIGEST_DOMAIN_V1);
    digest.update((payloads.len() as u64).to_be_bytes());
    for payload in payloads {
        digest.update((payload.preimage.len() as u64).to_be_bytes());
        digest.update(&payload.preimage);
    }
    Ok(digest.finalize().into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignOutcome {
    Signature(Vec<u8>),
    ApprovalPending { action_id: String, expires_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignBatchOutcome {
    Signatures(Vec<Vec<u8>>),
    ApprovalPending { action_id: String, expires_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvmTransaction {
    pub wallet: String,
    pub chain: String,
    pub to: String,
    pub value_wei: String,
    pub data_hex: String,
    pub nonce: Option<u64>,
    pub max_fee_per_gas: Option<String>,
    pub max_priority_fee_per_gas: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxApproval {
    pub action_id: String,
    pub expires_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransaction {
    pub outbox_id: String,
    pub plan_md: String,
    pub approval: Option<OutboxApproval>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxInspection {
    pub outbox_id: String,
    pub state: String,
    pub tx_hash: Option<String>,
    pub receipt_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostStatus {
    NotFound,
    Denied,
    Invalid,
    Backend,
    BufferTooSmall { needed: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdkError {
    Host(HostStatus),
    Message(String),
}

impl SdkError {
    pub fn message(&self) -> String {
        match self {
            SdkError::Host(HostStatus::NotFound) => "not found".into(),
            SdkError::Host(HostStatus::Denied) => "denied".into(),
            SdkError::Host(HostStatus::Invalid) => "invalid".into(),
            SdkError::Host(HostStatus::Backend) => "backend error".into(),
            SdkError::Host(HostStatus::BufferTooSmall { needed }) => {
                format!("buffer too small: needs {needed} bytes")
            }
            SdkError::Message(message) => message.clone(),
        }
    }
}

impl core::fmt::Display for SdkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message())
    }
}

impl core::error::Error for SdkError {}

/// Validate the identifier used to address a Bloom wallet.
///
/// Wallet identifiers are Broker protocol tokens, not on-chain addresses.
/// Keeping this check in the SDK makes malformed route parameters fail before
/// a signing or custody request crosses the host boundary.
pub fn validate_wallet_id(value: &str) -> Result<&str, String> {
    if value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(
            "wallet must be a Bloom wallet id, not an on-chain address; use the id under /bloom/wallets/<id>"
                .into(),
        );
    }
    if value.is_empty() || value.len() > 64 {
        return Err("wallet must be a Bloom wallet id containing 1-64 bytes".into());
    }
    if !value.as_bytes()[0].is_ascii_lowercase() {
        return Err(
            "wallet must be a Bloom wallet id starting with a lowercase ASCII letter".into(),
        );
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._/-".contains(&byte))
    {
        return Err(
            "wallet must be a Bloom wallet id using only lowercase ASCII letters, digits, '.', '_', '/', or '-'"
                .into(),
        );
    }
    Ok(value)
}

/// Children of `/wallets` that Bloom answers itself at every depth.
const RESERVED_WALLET_CHILDREN: &[&str] = &["registrations", "recoveries"];

/// Validate a wallet id that arrives from untrusted input — a request body or
/// stored state — before it becomes a VFS path segment or a store key.
///
/// This is Bloom's own wallet-name grammar, which is narrower than the wire
/// type: a new name must pass both Bloom's `[A-Za-z0-9_-]` check and the
/// `Token` rules (a lowercase first byte, then `[a-z0-9._/-]`), and the
/// intersection is `[a-z][a-z0-9_-]{0,63}`. So it rejects `/` and `.`, which
/// [`validate_wallet_id`] admits because `Token` does, and nothing is trimmed:
/// a padded id is refused, not repaired.
///
/// A route's own wallet does not need this. Bloom resolves `[wallet]` against
/// its projection before dispatch, and [`wallet_account`] reads that trusted
/// value. [`validate_wallet_id`] stays as wide as `Token`, for checks at the
/// wire boundary. Bloom never creates an id containing `.` or `/`, but only
/// this validator guarantees a safe path segment.
///
/// It also refuses the names Bloom's `/wallets` tree answers with its own
/// nodes at every depth, so `wallets/<name>/...` never reaches a wallet:
/// `registrations`, and `recoveries` since remote recovery (bloom#306).
/// `new` and `recover` are special only as the single path `wallets/<name>`,
/// so a wallet with either name still resolves beneath it.
pub fn validate_wallet_segment(value: &str) -> Result<&str, String> {
    if RESERVED_WALLET_CHILDREN.contains(&value) {
        return Err(format!(
            "{value:?} names one of Bloom's own /wallets nodes, not a wallet"
        ));
    }
    let bytes = value.as_bytes();
    let valid = (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        });
    if valid {
        Ok(value)
    } else {
        Err("wallet must be a Bloom wallet name: a lowercase letter, then up to 63 lowercase letters, digits, '_' or '-'".into())
    }
}

pub mod sdk {
    pub use super::{
        DispatchResponse, EvmTransaction, HostStatus, HttpRequest, HttpResponse, OutboxApproval,
        OutboxInspection, PayloadBatchSignRequest, PayloadSignItem, PayloadSignRequest,
        PetalKeyOutcome, PetalKeyRequest, SdkError, SignBatchOutcome, SignOutcome, SignSelector,
        StagedTransaction, payload_batch_digest,
    };
    use crate::bindings::bloom::chain::read as chain;
    use crate::bindings::bloom::env::runtime as env;
    use crate::bindings::bloom::http::fetch as http;
    use crate::bindings::bloom::key::derive as key;
    use crate::bindings::bloom::sign::signing as sign;
    use crate::bindings::bloom::store::kv as store;
    use crate::bindings::bloom::tx::outbox as tx;
    use crate::bindings::bloom::vfs::readwrite as vfs;

    const STATE_NS: &str = "state";
    const SECRET_NS: &str = "secrets";

    pub fn http_fetch(req: &HttpRequest, max_bytes: usize) -> Result<HttpResponse, SdkError> {
        set_host_reason(None);
        let resp = http::fetch(&http::Request {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: req.headers.clone(),
            body: req.body.clone(),
        })
        .map_err(host_err)?;
        if resp.body.len() > max_bytes {
            // The fetch itself succeeded, so no host reason belongs to this
            // error; clearing stops an older one from being read against it.
            set_host_reason(None);
            return Err(SdkError::Host(HostStatus::BufferTooSmall {
                needed: resp.body.len(),
            }));
        }
        Ok(HttpResponse {
            status: resp.status,
            headers: resp.headers,
            body: resp.body,
        })
    }

    pub fn request_key(request_jcs: &[u8]) -> Result<Vec<u8>, SdkError> {
        set_host_reason(None);
        let request: PetalKeyRequest = serde_json::from_slice(request_jcs)
            .map_err(|error| SdkError::Message(format!("decode Petal key request: {error}")))?;
        super::validate_wallet_id(&request.wallet_id).map_err(SdkError::Message)?;
        key::request(request_jcs).map_err(host_err)
    }

    pub fn derive_key(request: &PetalKeyRequest) -> Result<PetalKeyOutcome, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(&request.wallet_id).map_err(SdkError::Message)?;
        let request_jcs = serde_jcs::to_vec(request)
            .map_err(|error| SdkError::Message(format!("encode Petal key request: {error}")))?;
        let outcome = request_key(&request_jcs)?;
        serde_json::from_slice(&outcome)
            .map_err(|error| SdkError::Message(format!("decode Petal key outcome: {error}")))
    }

    pub fn sign_payload(req: &PayloadSignRequest) -> Result<SignOutcome, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(&req.wallet).map_err(SdkError::Message)?;
        let request = sign::PayloadSignRequest {
            wallet: req.wallet.clone(),
            preimage: req.preimage.clone(),
            claimed_hash: req.claimed_hash.to_vec(),
            signature_algorithm: req.signature_algorithm.clone(),
            operation_class: req.operation_class.clone(),
            petal_use_claim_jcs: req.petal_use_claim_jcs.clone(),
            claim_assurance_evidence: req.claim_assurance_evidence.clone(),
            approval_hint: req.approval_hint.clone(),
            action: req.action.clone(),
            advisory: req.advisory.clone(),
            selector: selector(&req.selector),
            key_ref_jcs: req.key_ref_jcs.clone(),
        };
        match sign::sign_payload(&request).map_err(host_err)? {
            sign::SignResult::Signature(signature) => Ok(SignOutcome::Signature(signature)),
            sign::SignResult::ApprovalPending(approval) => Ok(SignOutcome::ApprovalPending {
                action_id: approval.action_id,
                expires_ms: approval.expires_ms,
            }),
        }
    }

    pub fn sign_payload_batch(req: &PayloadBatchSignRequest) -> Result<SignBatchOutcome, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(&req.wallet).map_err(SdkError::Message)?;
        super::validate_payload_batch(&req.payloads)?;
        let request = sign::PayloadBatchSignRequest {
            wallet: req.wallet.clone(),
            payloads: req
                .payloads
                .iter()
                .map(|payload| sign::PayloadSignItem {
                    preimage: payload.preimage.clone(),
                    claimed_hash: payload.claimed_hash.to_vec(),
                })
                .collect(),
            signature_algorithm: req.signature_algorithm.clone(),
            operation_class: req.operation_class.clone(),
            petal_use_claim_jcs: req.petal_use_claim_jcs.clone(),
            claim_assurance_evidence: req.claim_assurance_evidence.clone(),
            approval_hint: req.approval_hint.clone(),
            action: req.action.clone(),
            advisory: req.advisory.clone(),
            selector: selector(&req.selector),
            key_ref_jcs: req.key_ref_jcs.clone(),
        };
        match sign::sign_payload_batch(&request).map_err(host_err)? {
            sign::SignBatchResult::Signatures(signatures) => {
                if signatures.len() != req.payloads.len() {
                    return Err(SdkError::Message(
                        "host returned the wrong payload batch signature count".into(),
                    ));
                }
                Ok(SignBatchOutcome::Signatures(signatures))
            }
            sign::SignBatchResult::ApprovalPending(approval) => {
                Ok(SignBatchOutcome::ApprovalPending {
                    action_id: approval.action_id,
                    expires_ms: approval.expires_ms,
                })
            }
        }
    }

    fn selector(selector: &SignSelector) -> sign::Selector {
        match selector {
            SignSelector::Exact => sign::Selector::Exact,
            SignSelector::Reusable => sign::Selector::Reusable,
        }
    }

    pub fn tx_stage(req: &EvmTransaction) -> Result<StagedTransaction, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(&req.wallet).map_err(SdkError::Message)?;
        tx::stage(&tx::EvmTransaction {
            wallet: req.wallet.clone(),
            chain: req.chain.clone(),
            to: req.to.clone(),
            value_wei: req.value_wei.clone(),
            data_hex: req.data_hex.clone(),
            nonce: req.nonce,
            max_fee_per_gas: req.max_fee_per_gas.clone(),
            max_priority_fee_per_gas: req.max_priority_fee_per_gas.clone(),
        })
        .map(staged_transaction)
        .map_err(host_err)
    }

    pub fn tx_confirm(
        wallet: &str,
        chain_name: &str,
        outbox_id: &str,
        acknowledge_warnings: bool,
    ) -> Result<StagedTransaction, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(wallet).map_err(SdkError::Message)?;
        tx::confirm(wallet, chain_name, outbox_id, acknowledge_warnings)
            .map(staged_transaction)
            .map_err(host_err)
    }

    pub fn tx_inspect(
        wallet: &str,
        chain_name: &str,
        outbox_id: &str,
    ) -> Result<OutboxInspection, SdkError> {
        set_host_reason(None);
        super::validate_wallet_id(wallet).map_err(SdkError::Message)?;
        tx::inspect(wallet, chain_name, outbox_id)
            .map(|inspection| OutboxInspection {
                outbox_id: inspection.outbox_id,
                state: inspection.state,
                tx_hash: inspection.tx_hash,
                receipt_json: inspection.receipt_json,
            })
            .map_err(host_err)
    }

    pub fn chain_read(
        chain_name: &str,
        method: &str,
        params_json: &str,
    ) -> Result<String, SdkError> {
        set_host_reason(None);
        chain::call(&chain::Request {
            chain: chain_name.into(),
            method: method.into(),
            params_json: params_json.into(),
        })
        .map(|response| response.result_json)
        .map_err(host_err)
    }

    pub fn store_get(key: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(key, false);
        let Some(bytes) = store::get(namespace, key).map_err(host_err)? else {
            return Err(SdkError::Host(HostStatus::NotFound));
        };
        if bytes.len() > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall {
                needed: bytes.len(),
            }));
        }
        Ok(bytes)
    }

    pub fn store_put(key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(key, secret);
        store::put(namespace, key, value, namespace == SECRET_NS).map_err(host_err)
    }

    pub fn store_put_new(key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(key, secret);
        store::put_new(namespace, key, value, namespace == SECRET_NS).map_err(host_err)
    }

    pub fn store_del(key: &str) -> Result<(), SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(key, false);
        store::delete(namespace, key).map_err(host_err)
    }

    pub fn store_del_if_value(key: &str, expected: &[u8]) -> Result<(), SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(key, false);
        store::delete_if_value(namespace, key, expected).map_err(host_err)
    }

    pub fn store_list(prefix: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        set_host_reason(None);
        let namespace = namespace_for_key(prefix, false);
        let keys = store::list(namespace, prefix).map_err(host_err)?;
        let size = keys.iter().map(|key| key.len()).sum::<usize>();
        if size > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall { needed: size }));
        }
        Ok(keys)
    }

    pub fn vfs_read(path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        set_host_reason(None);
        let bytes = vfs::read(path).map_err(host_err)?;
        if bytes.len() > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall {
                needed: bytes.len(),
            }));
        }
        Ok(bytes)
    }

    pub fn vfs_write(path: &str, body: &[u8]) -> Result<(), SdkError> {
        set_host_reason(None);
        vfs::write(path, body).map_err(host_err)
    }

    pub fn vfs_list(path: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        set_host_reason(None);
        let _ = vfs::lookup(path).map_err(host_err)?;
        let entries = vfs::list(path).map_err(host_err)?;
        let size = entries.iter().map(|entry| entry.name.len()).sum::<usize>();
        if size > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall { needed: size }));
        }
        Ok(entries.into_iter().map(|entry| entry.name).collect())
    }

    pub fn now_ms() -> u64 {
        set_host_reason(None);
        env::now_ms().unwrap_or(0)
    }

    pub fn try_now_ms() -> Result<u64, SdkError> {
        set_host_reason(None);
        env::now_ms().map_err(host_err)
    }

    pub fn random_bytes(len: usize) -> Result<Vec<u8>, SdkError> {
        set_host_reason(None);
        let len = u32::try_from(len).map_err(|_| SdkError::Host(HostStatus::Invalid))?;
        env::random_bytes(len).map_err(host_err)
    }

    pub fn runtime_setting(key: &str) -> Result<Option<String>, SdkError> {
        set_host_reason(None);
        env::setting(key).map_err(host_err)
    }

    fn staged_transaction(staged: tx::StagedTransaction) -> StagedTransaction {
        StagedTransaction {
            outbox_id: staged.outbox_id,
            plan_md: staged.plan_md,
            approval: staged.approval.map(|approval| OutboxApproval {
                action_id: approval.action_id,
                expires_ms: approval.expires_ms,
            }),
        }
    }

    fn namespace_for_key(key: &str, secret: bool) -> &'static str {
        if secret || key == "creds" || key.starts_with("creds/") {
            SECRET_NS
        } else {
            STATE_NS
        }
    }

    #[cfg(test)]
    mod namespace_tests {
        use super::{SECRET_NS, STATE_NS, namespace_for_key};

        #[test]
        fn credential_keys_round_trip_through_the_secret_namespace() {
            assert_eq!(namespace_for_key("creds/wallet/clob.json", true), SECRET_NS);
            assert_eq!(
                namespace_for_key("creds/wallet/clob.json", false),
                SECRET_NS
            );
            assert_eq!(namespace_for_key("creds", false), SECRET_NS);
        }

        #[test]
        fn ordinary_state_keys_remain_in_the_state_namespace() {
            assert_eq!(
                namespace_for_key("trade/wallet/draft.json", false),
                STATE_NS
            );
            assert_eq!(namespace_for_key("credentials/lookalike", false), STATE_NS);
        }
    }

    #[cfg(test)]
    mod host_reason_tests {
        use super::super::{HostStatus, SdkError};
        use super::{host_err, take_host_reason};

        /// The Machine denies exact signing with the gate that refused. The
        /// classification stays exactly what it was, so no existing route
        /// changes behaviour; the reason is what used to be lost.
        #[test]
        fn a_denial_keeps_the_gate_that_refused_it() {
            let _ = take_host_reason();
            let error = host_err(
                "denied: SELECTOR_MISMATCH: wallet policy must allow destination exact on evm-1"
                    .into(),
            );
            assert_eq!(error, SdkError::Host(HostStatus::Denied));
            assert_eq!(
                take_host_reason().as_deref(),
                Some("SELECTOR_MISMATCH: wallet policy must allow destination exact on evm-1")
            );
        }

        /// Taken once. A later read cannot pick up a reason belonging to an
        /// earlier failure and report it against an unrelated call.
        #[test]
        fn a_reason_is_taken_only_once() {
            let _ = take_host_reason();
            assert_eq!(
                host_err("denied: policy".into()),
                SdkError::Host(HostStatus::Denied)
            );
            assert_eq!(take_host_reason().as_deref(), Some("policy"));
            assert_eq!(take_host_reason(), None);
        }

        /// Every host failure replaces the previous reason, including one that
        /// has none, so a labelled-only refusal cannot inherit an explanation
        /// from the call before it.
        #[test]
        fn a_reasonless_failure_clears_the_previous_reason() {
            let _ = take_host_reason();
            host_err("denied: policy".into());
            assert_eq!(
                host_err("permission denied".into()),
                SdkError::Host(HostStatus::Denied)
            );
            assert_eq!(take_host_reason(), None);
        }

        #[test]
        fn backend_words_do_not_reclassify_the_error_or_leave_a_reason() {
            host_err("denied: previous policy".into());
            let message = "backend: invalid response: permission information not found";
            assert_eq!(host_err(message.into()), SdkError::Message(message.into()));
            assert_eq!(take_host_reason(), None);
        }

        #[test]
        fn sdk_local_failure_clears_a_previous_host_reason() {
            host_err("denied: previous policy".into());
            assert!(super::request_key(b"{}").is_err());
            assert_eq!(take_host_reason(), None);
        }

        #[test]
        fn classified_reason_is_bounded_without_splitting_utf8() {
            host_err(format!("invalid: {}", "é".repeat(600)));
            let reason = take_host_reason().unwrap();
            assert_eq!(reason.chars().count(), 512);
            assert!(reason.chars().all(|c| c == 'é'));
        }

        #[test]
        fn an_unclassified_host_error_still_reaches_the_route_intact() {
            let _ = take_host_reason();
            let error = host_err("backend: broker unreachable".into());
            assert_eq!(
                error,
                SdkError::Message("backend: broker unreachable".into())
            );
            assert_eq!(error.message(), "backend: broker unreachable");
        }

        /// The reason a route reads must belong to the call it just made.
        ///
        /// An unclassified failure returns early with the whole message, so it
        /// is easy to leave the slot untouched — and then a route asking why
        /// this call failed is handed the refusal from the last one, which is
        /// a wrong answer rather than a missing one.
        #[test]
        fn an_unclassified_failure_does_not_leave_the_previous_reason_behind() {
            let _ = take_host_reason();
            host_err("denied: old policy refusal".into());
            host_err("backend: broker unreachable".into());
            assert_eq!(take_host_reason(), None);
            assert_eq!(take_host_reason(), None);
        }
    }

    thread_local! {
        static LAST_HOST_REASON: core::cell::RefCell<Option<String>> =
            const { core::cell::RefCell::new(None) };
    }

    /// The reason the host gave for the most recent failed host call, removing
    /// it so it can be read only once.
    ///
    /// `SdkError::Host(HostStatus::Denied)` says a call was refused but not
    /// which gate refused it. The host does send that: the Machine formats
    /// exact-signing refusals as `denied: <reason>`, naming the wallet policy,
    /// the package provenance, the key mismatch or the stale approval that
    /// stopped the call. Classifying the message threw the rest away, leaving
    /// the reason only in the Machine's log — which an operator inside a
    /// container, or an evaluated agent, cannot read.
    ///
    /// Read it when the error is an `SdkError::Host(..)`, and attach what it
    /// returns to the diagnostic the route reports. Every path that produces
    /// one replaces this slot first, so the reason always belongs to the call
    /// whose error you are holding, and is `None` when the host gave no reason
    /// or when it has already been taken.
    ///
    /// An `SdkError::Message` carries its own text, so there is nothing here to
    /// add to it. Some of those are raised by the SDK before any host call —
    /// a malformed request, an invalid wallet id — and those do not touch the
    /// slot, so reading it against one would report an earlier host failure.
    ///
    /// Scope: a route component is instantiated per dispatch and the guest is
    /// single-threaded, so there is never more than one host call in flight and
    /// a reason cannot arrive from another request or another wallet. Taking
    /// rather than peeking means a later unrelated read cannot pick up a reason
    /// belonging to an earlier failure.
    pub fn take_host_reason() -> Option<String> {
        LAST_HOST_REASON.with(|slot| slot.borrow_mut().take())
    }

    /// Replace the slot so it describes the failure being returned right now.
    ///
    /// Every path that returns an `SdkError::Host` calls this, including the
    /// ones with nothing to record. Leaving the slot alone instead would hand
    /// a route the *previous* call's reason to explain this one.
    fn set_host_reason(reason: Option<String>) {
        LAST_HOST_REASON.with(|slot| *slot.borrow_mut() = reason);
    }

    fn host_err(message: String) -> SdkError {
        set_host_reason(None);
        let (label, reason) = message.split_once(": ").unwrap_or((&message, ""));
        let status = match label.to_ascii_lowercase().as_str() {
            "not found" => HostStatus::NotFound,
            "denied" | "permission denied" => HostStatus::Denied,
            "invalid" => HostStatus::Invalid,
            // Words in a backend explanation are not a status prefix.
            _ => return SdkError::Message(message),
        };
        if !reason.is_empty() {
            set_host_reason(Some(
                reason
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(512)
                    .collect(),
            ));
        }
        SdkError::Host(status)
    }
}

#[macro_export]
macro_rules! route_file {
    (spec: $spec:expr, list: $children:expr $(,)?) => {
        pub struct Route;

        impl $crate::RawGuest for Route {
            fn metadata(ctx: $crate::RawCtx) -> Result<$crate::RouteMeta, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_metadata(&ctx, $spec)
            }

            fn lookup(ctx: $crate::RawCtx) -> Result<$crate::Entry, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_lookup(&ctx, $spec)
            }

            fn list(ctx: $crate::RawCtx) -> Result<Vec<$crate::Entry>, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let children = $children;
                $crate::framework_list(&ctx, children)
            }

            fn read(_ctx: $crate::RawCtx) -> Result<Vec<u8>, $crate::RouteError> {
                Err($crate::RouteError::Invalid("not a file".into()))
            }

            fn write(_ctx: $crate::RawCtx, _body: Vec<u8>) -> Result<(), $crate::RouteError> {
                Err($crate::RouteError::Denied("path is not writable".into()))
            }
        }
    };
    (spec: $spec:expr, fallible_list: $children:expr $(,)?) => {
        pub struct Route;

        impl $crate::RawGuest for Route {
            fn metadata(ctx: $crate::RawCtx) -> Result<$crate::RouteMeta, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_metadata(&ctx, $spec)
            }

            fn lookup(ctx: $crate::RawCtx) -> Result<$crate::Entry, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_lookup(&ctx, $spec)
            }

            fn list(ctx: $crate::RawCtx) -> Result<Vec<$crate::Entry>, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let children = $children;
                $crate::framework_fallible_list(&ctx, children)
            }

            fn read(_ctx: $crate::RawCtx) -> Result<Vec<u8>, $crate::RouteError> {
                Err($crate::RouteError::Invalid("not a file".into()))
            }

            fn write(_ctx: $crate::RawCtx, _body: Vec<u8>) -> Result<(), $crate::RouteError> {
                Err($crate::RouteError::Denied("path is not writable".into()))
            }
        }
    };
    (spec: $spec:expr, ctx_list: $children:expr $(,)?) => {
        pub struct Route;

        impl $crate::RawGuest for Route {
            fn metadata(ctx: $crate::RawCtx) -> Result<$crate::RouteMeta, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_metadata(&ctx, $spec)
            }

            fn lookup(ctx: $crate::RawCtx) -> Result<$crate::Entry, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_lookup(&ctx, $spec)
            }

            fn list(ctx: $crate::RawCtx) -> Result<Vec<$crate::Entry>, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let children = $children;
                $crate::framework_fallible_list(&ctx, children(&ctx))
            }

            fn read(_ctx: $crate::RawCtx) -> Result<Vec<u8>, $crate::RouteError> {
                Err($crate::RouteError::Invalid("not a file".into()))
            }

            fn write(_ctx: $crate::RawCtx, _body: Vec<u8>) -> Result<(), $crate::RouteError> {
                Err($crate::RouteError::Denied("path is not writable".into()))
            }
        }
    };
    (spec: $spec:expr, read: $read:expr $(,)?) => {
        pub struct Route;

        impl $crate::RawGuest for Route {
            fn metadata(ctx: $crate::RawCtx) -> Result<$crate::RouteMeta, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_metadata(&ctx, $spec)
            }

            fn lookup(ctx: $crate::RawCtx) -> Result<$crate::Entry, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_lookup(&ctx, $spec)
            }

            fn list(_ctx: $crate::RawCtx) -> Result<Vec<$crate::Entry>, $crate::RouteError> {
                Err($crate::RouteError::Invalid("not a directory".into()))
            }

            fn read(ctx: $crate::RawCtx) -> Result<Vec<u8>, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let read = $read;
                $crate::framework_read(read(&ctx))
            }

            fn write(_ctx: $crate::RawCtx, _body: Vec<u8>) -> Result<(), $crate::RouteError> {
                Err($crate::RouteError::Denied("path is not writable".into()))
            }
        }
    };
    (spec: $spec:expr, read: $read:expr, write: $write:expr $(,)?) => {
        pub struct Route;

        impl $crate::RawGuest for Route {
            fn metadata(ctx: $crate::RawCtx) -> Result<$crate::RouteMeta, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_metadata(&ctx, $spec)
            }

            fn lookup(ctx: $crate::RawCtx) -> Result<$crate::Entry, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                $crate::framework_lookup(&ctx, $spec)
            }

            fn list(_ctx: $crate::RawCtx) -> Result<Vec<$crate::Entry>, $crate::RouteError> {
                Err($crate::RouteError::Invalid("not a directory".into()))
            }

            fn read(ctx: $crate::RawCtx) -> Result<Vec<u8>, $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let read = $read;
                $crate::framework_read(read(&ctx))
            }

            fn write(ctx: $crate::RawCtx, body: Vec<u8>) -> Result<(), $crate::RouteError> {
                let ctx = $crate::Ctx::bind::<crate::__PetalRouteIdentity>(ctx);
                let write = $write;
                $crate::framework_write(write(&ctx, &body))
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFileKind {
    Dir,
    File,
    WritableFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteSpec {
    kind: RouteFileKind,
    cache_ttl_ms: Option<u64>,
    side_effecting_read: bool,
    write_async: bool,
    required_caps: &'static [&'static str],
    sign_intent: Option<&'static str>,
}

impl RouteSpec {
    const fn dir() -> Self {
        Self::new(RouteFileKind::Dir)
    }

    const fn file() -> Self {
        Self::new(RouteFileKind::File)
    }

    const fn writable() -> Self {
        Self::new(RouteFileKind::WritableFile)
    }

    const fn new(kind: RouteFileKind) -> Self {
        Self {
            kind,
            cache_ttl_ms: Some(30_000),
            side_effecting_read: false,
            write_async: false,
            required_caps: CAPS_NONE,
            sign_intent: None,
        }
    }

    pub const fn caps(mut self, caps: &'static [&'static str]) -> Self {
        self.required_caps = caps;
        self
    }

    const fn sign_intent(mut self, intent: &'static str) -> Self {
        self.sign_intent = Some(intent);
        self
    }

    const fn ttl(mut self, ttl: Option<u64>) -> Self {
        self.cache_ttl_ms = ttl;
        self
    }

    const fn side_effecting_read(mut self, value: bool) -> Self {
        self.side_effecting_read = value;
        self
    }

    const fn write_async(mut self, value: bool) -> Self {
        self.write_async = value;
        self
    }
}

const CAPS_NONE: &[&str] = &[];
const CAPS_HTTP: &[&str] = &["bloom:http"];
const CAPS_STORE: &[&str] = &["bloom:store"];
const CAPS_STORE_VFS_READ: &[&str] = &["bloom:store", "bloom:vfs.read"];
const CAPS_HTTP_VFS_READ: &[&str] = &["bloom:http", "bloom:vfs.read"];
const CAPS_HTTP_STORE_VFS_READ: &[&str] = &["bloom:http", "bloom:store", "bloom:vfs.read"];
const CAPS_HTTP_STORE_SIGN_VFS: &[&str] = &[
    "bloom:http",
    "bloom:store",
    "bloom:sign",
    "bloom:tx.outbox",
    "bloom:chain",
    "bloom:vfs.read",
    "bloom:vfs.write",
];

pub fn static_dir_spec() -> RouteSpec {
    RouteSpec::dir()
}

pub fn store_dir_spec() -> RouteSpec {
    RouteSpec::dir().caps(CAPS_STORE_VFS_READ)
}

pub fn http_dir_spec() -> RouteSpec {
    RouteSpec::dir().caps(CAPS_HTTP)
}

pub fn static_read_spec() -> RouteSpec {
    RouteSpec::file()
}

pub fn http_read_spec(ttl_ms: u64) -> RouteSpec {
    RouteSpec::file().caps(CAPS_HTTP).ttl(Some(ttl_ms))
}

pub fn store_read_spec() -> RouteSpec {
    RouteSpec::file().caps(CAPS_STORE)
}

pub fn wallet_http_read_spec(ttl_ms: u64) -> RouteSpec {
    RouteSpec::file().caps(CAPS_HTTP_VFS_READ).ttl(Some(ttl_ms))
}

pub fn account_read_spec() -> RouteSpec {
    RouteSpec::file()
        .caps(CAPS_HTTP_STORE_VFS_READ)
        .ttl(Some(5_000))
}

pub fn chain_read_spec() -> RouteSpec {
    RouteSpec::file()
        .caps(CAPS_HTTP_STORE_SIGN_VFS)
        .ttl(None)
        .side_effecting_read(true)
}

pub fn write_spec() -> RouteSpec {
    RouteSpec::writable()
        .caps(CAPS_HTTP_STORE_SIGN_VFS)
        .ttl(None)
        .write_async(true)
}

pub fn signing_write_spec(intent: &'static str) -> RouteSpec {
    write_spec().sign_intent(intent)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteChild {
    name: String,
    kind: RouteFileKind,
}

pub fn current_route_path(ctx: &Ctx) -> &'static str {
    ctx.identity_path
}

pub fn current_route_canonical_path(ctx: &Ctx) -> &'static str {
    ctx.identity_canonical_path
}

pub fn framework_metadata(ctx: &Ctx, spec: RouteSpec) -> Result<RouteMeta, RouteError> {
    let description = if ctx
        .identity_params
        .iter()
        .any(|(name, _)| *name == "wallet")
    {
        format!(
            "Petal route {}. Path parameter [wallet] is a Bloom wallet id under /bloom/wallets/<id>, not an on-chain address.",
            current_route_path(ctx)
        )
    } else {
        format!("Petal route {}", current_route_path(ctx))
    };
    Ok(RouteMeta {
        kind: match spec.kind {
            RouteFileKind::Dir => EntryKind::Dir,
            RouteFileKind::File | RouteFileKind::WritableFile => EntryKind::File,
        },
        mode: match spec.kind {
            RouteFileKind::Dir => 0o755,
            RouteFileKind::File => 0o444,
            RouteFileKind::WritableFile => 0o644,
        },
        cache_ttl_ms: spec.cache_ttl_ms,
        side_effecting_read: spec.side_effecting_read,
        write_async: spec.write_async,
        description: Some(description),
        consent_summary: None,
        required_caps: spec
            .required_caps
            .iter()
            .map(|cap| (*cap).to_string())
            .collect(),
        sign_intent: spec.sign_intent.map(str::to_string),
        executable: false,
    })
}

pub fn framework_lookup(ctx: &Ctx, spec: RouteSpec) -> Result<Entry, RouteError> {
    let relative = route_relative(ctx);
    Ok(framework_entry(entry_name(&relative), spec.kind))
}

pub fn framework_list(_ctx: &Ctx, children: Vec<RouteChild>) -> Result<Vec<Entry>, RouteError> {
    Ok(children
        .into_iter()
        .filter(|child| is_safe_segment(&child.name))
        .map(|child| framework_entry(&child.name, child.kind))
        .collect())
}

pub fn framework_fallible_list(
    ctx: &Ctx,
    children: Result<Vec<RouteChild>, DispatchResponse>,
) -> Result<Vec<Entry>, RouteError> {
    match children {
        Ok(children) => framework_list(ctx, children),
        Err(DispatchResponse::Error { code, message }) => Err(route_error(code, message)),
        Err(_) => Err(RouteError::Backend(
            "list returned non-list response".into(),
        )),
    }
}

pub fn framework_read(resp: DispatchResponse) -> Result<Vec<u8>, RouteError> {
    match resp {
        DispatchResponse::Read(bytes) => Ok(bytes),
        DispatchResponse::Error { code, message } => Err(route_error(code, message)),
        _ => Err(RouteError::Backend(
            "read returned non-read response".into(),
        )),
    }
}

pub fn framework_write(resp: DispatchResponse) -> Result<(), RouteError> {
    match resp {
        DispatchResponse::Write => Ok(()),
        DispatchResponse::Error { code, message } => Err(route_error(code, message)),
        _ => Err(RouteError::Backend(
            "write returned non-write response".into(),
        )),
    }
}

pub fn route_relative(ctx: &Ctx) -> String {
    if ctx.path.is_empty() {
        return current_route_canonical_path(ctx).to_string();
    }
    metadata_path(&ctx.path)
}

pub fn route_param<'a>(ctx: &'a Ctx, name: &str) -> Option<&'a str> {
    ctx.params
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
}

pub fn route_segment<'a>(ctx: &'a Ctx, index: usize) -> Option<&'a str> {
    split(&ctx.path).get(index).copied()
}

pub fn param<'a>(ctx: &'a Ctx, name: &str) -> Result<&'a str, DispatchResponse> {
    route_param(ctx, name)
        .or_else(|| route_generated_param(ctx, name))
        .ok_or_else(|| route_invalid(format!("missing {name}")))
}

/// Return the reserved `[wallet]` route parameter as a validated Bloom wallet
/// identifier rather than an on-chain address.
pub fn wallet_param(ctx: &Ctx) -> Result<&str, DispatchResponse> {
    let value = param(ctx, "wallet")?;
    validate_wallet_id(value).map_err(route_invalid)
}

/// The `$index.rs` for a `[wallet]` directory whose children are account
/// numbers.
///
/// Bloom enumerates a wallet's accounts from its own authenticated projection
/// and never dispatches this listing to the guest, so the only correct answer a
/// Petal can give is "nothing". Nine byte-identical copies of this file existed
/// across four Petals before this macro, in three different wordings.
#[macro_export]
macro_rules! account_listing {
    () => {
        $crate::route_file!(spec: $crate::static_dir_spec(), list: Vec::new());
    };
}

// ---------------------------------------------------------------------------
// Wallet account identity
// ---------------------------------------------------------------------------

/// How many bytes an address leaf may occupy. Bloom writes a checksummed EVM
/// address or a base58 Solana address plus a newline; the bound is generous so
/// a projection change surfaces as a parse error, never a silent truncation.
pub const ADDRESS_MAX_BYTES: usize = 128;

/// The wallet and account number Bloom resolved for this dispatch.
///
/// Bloom canonicalises `[index]`, proves the account projection exists, and
/// injects the pair as trusted context before the guest runs, so this reports
/// what the host decided rather than re-validating a path capture. The fields
/// are private so that, in normal use, the pair stays the one the host
/// resolved. That is a discipline aid, not a security boundary:
/// [`testing::wallet_account`], or a [`RawCtx`] passed to [`Ctx::bind`], can
/// build any value. Bloom's host enforces the boundary: it refuses a guest VFS
/// path into any wallet account but the selected one, and signing for any
/// wallet but the trusted one.
///
/// One Petal serves several wallets by being dispatched once per wallet. Carry
/// this value into domain code instead of a bare `&str`, so each function
/// states which wallet it acts for and two of them cannot be transposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletAccount {
    wallet: String,
    number: u32,
}

/// Read the wallet and account Bloom selected for this dispatch.
///
/// There is deliberately no default: a route that is not account-mounted gets
/// an error rather than account 0. Bloom passes no `bloom.*` parameters to a route's
/// `metadata()`, so called from there it returns `-2` too.
pub fn wallet_account(ctx: &Ctx) -> Result<WalletAccount, DispatchResponse> {
    let wallet = route_param(ctx, "bloom.wallet")
        .ok_or_else(|| error(-2, "Bloom did not select a wallet for this route"))?;
    // Bloom injects `bloom.wallet` equal to the `[wallet]` capture, and its
    // runner refuses caller params named `bloom.*`, so these agree on every
    // context Bloom builds. Compare against the generated capture anyway, as
    // defence in depth: it is positional, so even a context that carried a
    // second `wallet` param could not shadow it.
    if route_generated_param(ctx, "wallet").is_some_and(|capture| capture != wallet) {
        return Err(error(-2, "Bloom did not select this wallet"));
    }
    if !is_safe_segment(wallet) || wallet.contains('/') {
        return Err(route_invalid(
            "selected wallet is not a single path segment",
        ));
    }
    let raw = route_param(ctx, "bloom.account")
        .ok_or_else(|| error(-2, "Bloom did not select an account for this route"))?;
    let number = raw
        .parse::<u32>()
        .map_err(|_| error(-4, format!("selected account {raw:?} is not a u32")))?;
    Ok(WalletAccount {
        wallet: wallet.to_owned(),
        number,
    })
}

impl WalletAccount {
    /// Construct one directly. Only [`crate::testing`] may do this: a route
    /// must go through [`wallet_account`] so the pair stays host-resolved.
    pub(crate) fn for_test(wallet: &str, number: u32) -> Self {
        Self {
            wallet: wallet.to_owned(),
            number,
        }
    }

    pub fn wallet(&self) -> &str {
        &self.wallet
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    /// `wallets/<wallet>/<number>` — the account directory.
    pub fn dir(&self) -> String {
        format!("wallets/{}/{}", self.wallet, self.number)
    }

    /// `wallets/<wallet>/<number>/<rest>`, refusing any `rest` that could leave
    /// the account directory.
    ///
    /// Every segment must pass [`is_safe_segment`], which rejects `.` and `..`.
    /// Checking per segment rather than whole-string matters: `"../0/address.evm"`
    /// is not equal to `".."`, so only the split form catches it. An empty,
    /// absolute or doubled-slash path yields an empty segment and fails too.
    pub fn path(&self, rest: &str) -> Result<String, DispatchResponse> {
        if !rest.split('/').all(is_safe_segment) {
            return Err(route_invalid(format!(
                "account path {rest:?} must be safe segments beneath the account"
            )));
        }
        Ok(format!("{}/{rest}", self.dir()))
    }

    /// `wallets/<wallet>/<number>/address.evm`.
    pub fn address_evm_path(&self) -> String {
        format!("{}/address.evm", self.dir())
    }

    /// `wallets/<wallet>/<number>/address.sol`.
    pub fn address_sol_path(&self) -> String {
        format!("{}/address.sol", self.dir())
    }

    /// This account's EVM address, read from Bloom's projection.
    pub fn address_evm(&self) -> Result<String, DispatchResponse> {
        self.address_evm_from(&mut HostVfs)
    }

    /// This account's Solana address, read from Bloom's projection.
    pub fn address_sol(&self) -> Result<String, DispatchResponse> {
        self.address_sol_from(&mut HostVfs)
    }

    /// [`Self::address_evm`] against a caller-supplied reader, for tests and for
    /// Petals that already own a host seam.
    pub fn address_evm_from<R: VfsRead>(&self, vfs: &mut R) -> Result<String, DispatchResponse> {
        let path = self.address_evm_path();
        let bytes = read_address(vfs, &path)?;
        parse_evm_address(&bytes).map_err(|reason| error(-4, format!("{path}: {reason}")))
    }

    /// [`Self::address_sol`] against a caller-supplied reader.
    pub fn address_sol_from<R: VfsRead>(&self, vfs: &mut R) -> Result<String, DispatchResponse> {
        let path = self.address_sol_path();
        let bytes = read_address(vfs, &path)?;
        parse_sol_address(&bytes).map_err(|reason| error(-4, format!("{path}: {reason}")))
    }
}

impl core::fmt::Display for WalletAccount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.wallet, self.number)
    }
}

/// The one VFS read the identity helpers need.
///
/// Host imports are `unreachable!()` off `wasm32`, so a function that calls one
/// cannot be unit-tested. Taking the reader as a parameter keeps the
/// composition testable and leaves [`WalletAccount::address_evm`] as a
/// one-liner whose only content is the choice of reader.
pub trait VfsRead {
    fn read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError>;
}

/// The real host.
pub struct HostVfs;

impl VfsRead for HostVfs {
    fn read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        sdk::vfs_read(path, max_bytes)
    }
}

/// An account with no key of that family projects no leaf for it, and Bloom
/// answers a missing `address.evm` or `address.sol` with `NotFound`. A leaf
/// Bloom does not project at all, reached through [`WalletAccount::path`] such
/// as `address.btc`, is `NotAFile`, which reaches the guest as `Invalid`. To a
/// caller both mean this account has no address there, so both report `-1`.
fn read_address<R: VfsRead>(vfs: &mut R, path: &str) -> Result<Vec<u8>, DispatchResponse> {
    vfs.read(path, ADDRESS_MAX_BYTES).map_err(|err| match err {
        SdkError::Host(HostStatus::NotFound | HostStatus::Invalid) => {
            error(-1, format!("{path}: this account has no address there"))
        }
        SdkError::Host(HostStatus::Denied) => error(-2, format!("{path}: denied")),
        other => error(-4, format!("{path}: {}", other.message())),
    })
}

/// Parse an `address.evm` leaf.
///
/// Shape only: the SDK has no keccak, so the EIP-55 checksum is not verified.
/// The address is returned exactly as Bloom projected it — casing intact — so a
/// caller parsing it into an address type still gets that check.
pub fn parse_evm_address(bytes: &[u8]) -> Result<String, String> {
    let value = core::str::from_utf8(bytes)
        .map_err(|_| "address is not UTF-8".to_owned())?
        .trim();
    let shaped = value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit());
    shaped
        .then(|| value.to_owned())
        .ok_or_else(|| format!("{value:?} is not an EVM address"))
}

/// Parse an `address.sol` leaf.
///
/// Shape only: the SDK has no base58 decoder, so this checks the alphabet and
/// the encoded length rather than decoding to 32 bytes.
pub fn parse_sol_address(bytes: &[u8]) -> Result<String, String> {
    const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let value = core::str::from_utf8(bytes)
        .map_err(|_| "address is not UTF-8".to_owned())?
        .trim();
    let shaped = (32..=44).contains(&value.len()) && value.chars().all(|ch| BASE58.contains(ch));
    shaped
        .then(|| value.to_owned())
        .ok_or_else(|| format!("{value:?} is not a Solana address"))
}

pub fn route_generated_param<'a>(ctx: &'a Ctx, name: &str) -> Option<&'a str> {
    ctx.identity_params
        .iter()
        .find_map(|(candidate, index)| (*candidate == name).then_some(*index))
        .and_then(|index| route_segment(ctx, index))
}

pub fn route_invalid(message: impl Into<String>) -> DispatchResponse {
    error(-3, message)
}

pub fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains('\\')
        && !segment.bytes().any(|byte| byte == 0)
}

pub fn split(relative: &str) -> Vec<&str> {
    if relative.is_empty() {
        Vec::new()
    } else {
        relative.split('/').collect()
    }
}

pub fn entry_name(relative: &str) -> &str {
    relative
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("")
}

pub fn error(code: i32, message: impl Into<String>) -> DispatchResponse {
    DispatchResponse::Error {
        code,
        message: message.into(),
    }
}

pub fn dir(name: impl Into<String>) -> RouteChild {
    RouteChild {
        name: name.into(),
        kind: RouteFileKind::Dir,
    }
}

pub fn file(name: impl Into<String>) -> RouteChild {
    RouteChild {
        name: name.into(),
        kind: RouteFileKind::File,
    }
}

pub fn writable(name: impl Into<String>) -> RouteChild {
    RouteChild {
        name: name.into(),
        kind: RouteFileKind::WritableFile,
    }
}

pub fn dirs(names: Vec<String>) -> Vec<RouteChild> {
    names.into_iter().map(dir).collect()
}

pub fn dir_names(names: &[&str]) -> Vec<RouteChild> {
    names.iter().map(|name| dir(*name)).collect()
}

pub fn files(names: &[&str]) -> Vec<RouteChild> {
    names.iter().map(|name| file(*name)).collect()
}

pub fn result_dirs(
    names: Result<Vec<String>, DispatchResponse>,
) -> Result<Vec<RouteChild>, DispatchResponse> {
    names.map(dirs)
}

pub fn framework_entry(name: &str, kind: RouteFileKind) -> Entry {
    Entry {
        name: name.into(),
        kind: match kind {
            RouteFileKind::Dir => EntryKind::Dir,
            RouteFileKind::File | RouteFileKind::WritableFile => EntryKind::File,
        },
        mode: match kind {
            RouteFileKind::Dir => 0o755,
            RouteFileKind::File => 0o444,
            RouteFileKind::WritableFile => 0o644,
        },
        size: Some(0),
        link_target: None,
    }
}

pub fn metadata_path(path: &str) -> String {
    match path {
        "$index" => String::new(),
        _ => path.strip_suffix("/$index").unwrap_or(path).to_string(),
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    struct Root;
    impl RouteIdentity for Root {
        const PATH: &'static str = "$index";
        const CANONICAL_PATH: &'static str = "";
        const PARAMS: &'static [(&'static str, usize)] = &[];
    }

    struct Nested;
    impl RouteIdentity for Nested {
        const PATH: &'static str = "trade/[wallet]/drafts/[id]/plan.md";
        const CANONICAL_PATH: &'static str = "trade/[wallet]/drafts/[id]/plan.md";
        const PARAMS: &'static [(&'static str, usize)] = &[("wallet", 1), ("id", 3)];
    }

    fn raw(path: &str, params: &[(&str, &str)]) -> RawCtx {
        RawCtx {
            petal_root: String::new(),
            package_hash: String::new(),
            path: path.into(),
            params: params
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into()))
                .collect(),
            actor: None,
        }
    }

    #[test]
    fn root_and_index_use_empty_canonical_fallback() {
        let ctx = Ctx::bind::<Root>(raw("", &[]));
        assert_eq!(route_relative(&ctx), "");
        assert_eq!(current_route_path(&ctx), "$index");
    }

    #[test]
    fn generated_params_support_multiple_and_repeated_lookups() {
        let ctx = Ctx::bind::<Nested>(raw("trade/0xabc/drafts/42/plan.md", &[]));
        assert_eq!(param(&ctx, "wallet").unwrap(), "0xabc");
        assert_eq!(param(&ctx, "id").unwrap(), "42");
        assert_eq!(param(&ctx, "wallet").unwrap(), "0xabc");
    }

    #[test]
    fn supplied_params_are_authoritative_and_partial_values_fall_back() {
        let ctx = Ctx::bind::<Nested>(raw(
            "trade/path-wallet/drafts/path-id/plan.md",
            &[("wallet", "supplied-wallet")],
        ));
        assert_eq!(param(&ctx, "wallet").unwrap(), "supplied-wallet");
        assert_eq!(param(&ctx, "id").unwrap(), "path-id");
    }

    #[test]
    fn wallet_params_use_bloom_wallet_ids() {
        let valid = Ctx::bind::<Nested>(raw("trade/alice-1/drafts/42/plan.md", &[]));
        assert_eq!(wallet_param(&valid).unwrap(), "alice-1");
        let metadata = framework_metadata(&valid, RouteSpec::file()).unwrap();
        let description = metadata.description.unwrap();
        assert!(description.contains("[wallet] is a Bloom wallet id"));
        assert!(description.contains("not an on-chain address"));

        let address = Ctx::bind::<Nested>(raw(
            "trade/0x0000000000000000000000000000000000000001/drafts/42/plan.md",
            &[],
        ));
        let Err(DispatchResponse::Error { code, message }) = wallet_param(&address) else {
            panic!("address-shaped wallet parameter must fail");
        };
        assert_eq!(code, -3);
        assert!(message.contains("on-chain address"));

        assert!(validate_wallet_id("Alice").is_err());
        assert!(validate_wallet_id("alice:1").is_err());
        assert!(validate_wallet_id(&format!("a{}", "1".repeat(64))).is_err());
    }

    #[test]
    fn raw_key_requests_cannot_bypass_wallet_validation() {
        let request = PetalKeyRequest {
            wallet_id: "0x0000000000000000000000000000000000000001".into(),
            key_slot: "session".into(),
            allowed_routes: vec!["r000001".into()],
            allowed_operation_classes: vec!["example.action".into()],
            allowed_crypto_suites: vec!["secp256k1-keccak256-recoverable".into()],
            maximum_lifetime_ms: 60_000,
        };
        let request_jcs = serde_jcs::to_vec(&request).unwrap();
        let Err(SdkError::Message(message)) = sdk::request_key(&request_jcs) else {
            panic!("raw request must reject an address-shaped wallet before the host call");
        };
        assert!(message.contains("on-chain address"));
    }

    #[test]
    fn absent_segments_and_unknown_params_return_the_existing_error() {
        let ctx = Ctx::bind::<Nested>(raw("trade", &[]));
        assert!(matches!(
            param(&ctx, "wallet"),
            Err(DispatchResponse::Error { code: -3, .. })
        ));
        assert!(matches!(
            param(&ctx, "unknown"),
            Err(DispatchResponse::Error { code: -3, .. })
        ));
    }

    #[test]
    fn payload_batch_digest_binds_order_and_boundaries() {
        let first = PayloadSignItem {
            preimage: b"a".to_vec(),
            claimed_hash: [1; 32],
        };
        let second = PayloadSignItem {
            preimage: b"bc".to_vec(),
            claimed_hash: [2; 32],
        };
        assert_ne!(
            payload_batch_digest(&[first.clone(), second.clone()]).unwrap(),
            payload_batch_digest(&[second, first]).unwrap()
        );
        assert_ne!(
            payload_batch_digest(&[
                PayloadSignItem {
                    preimage: b"a".to_vec(),
                    claimed_hash: [1; 32],
                },
                PayloadSignItem {
                    preimage: b"bc".to_vec(),
                    claimed_hash: [2; 32],
                },
            ])
            .unwrap(),
            payload_batch_digest(&[
                PayloadSignItem {
                    preimage: b"ab".to_vec(),
                    claimed_hash: [1; 32],
                },
                PayloadSignItem {
                    preimage: b"c".to_vec(),
                    claimed_hash: [2; 32],
                },
            ])
            .unwrap()
        );
        assert!(payload_batch_digest(&[]).is_err());
    }
}

#[cfg(test)]
mod account_tests {
    use super::testing::{MapVfs, RouteCtx};
    use super::*;

    struct Swap;
    impl RouteIdentity for Swap {
        const PATH: &'static str = "swaps/[wallet]/[index]/new";
        const CANONICAL_PATH: &'static str = "swaps/[wallet]/[index]/new";
        const PARAMS: &'static [(&'static str, usize)] = &[("wallet", 1), ("index", 2)];
    }

    fn ctx(path: &str, wallet: &str, number: u32) -> Ctx {
        RouteCtx::new(path).account(wallet, number).bind::<Swap>()
    }

    fn code(response: &DispatchResponse) -> i32 {
        match response {
            DispatchResponse::Error { code, .. } => *code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn a_wallet_segment_is_exactly_bloom_wallet_name_grammar() {
        for name in ["a", "main", "alice-1", "trading_2", "a0", &"a".repeat(64)] {
            assert_eq!(validate_wallet_segment(name), Ok(name), "{name:?}");
        }
        // Escape attempts, carried over from the Robinhood Petal's own table.
        for name in ["", "..", "../wallets", "a/b", "a\0b", &"a".repeat(65)] {
            assert!(validate_wallet_segment(name).is_err(), "{name:?}");
        }
        // Names Bloom cannot create: Token forbids uppercase anywhere and a
        // non-letter start, and Bloom's creation check forbids `.` and `/`.
        for name in [
            "Alice",
            "A1",
            "aLice",
            "alicE",
            "a\\b",
            "alic\u{e9}",
            "a:b",
            "1alice",
            "-a",
            "_a",
            "alice.vault",
            "a.",
            " alice",
            "alice ",
            "al ice",
        ] {
            assert!(validate_wallet_segment(name).is_err(), "{name:?}");
        }
        assert!(validate_wallet_segment(&format!("0x{}", "ab".repeat(20))).is_err());
    }

    #[test]
    fn a_wallet_segment_is_never_one_of_bloom_s_own_wallet_nodes() {
        // Bloom answers these itself at every depth under /wallets.
        for name in ["registrations", "recoveries"] {
            assert!(validate_wallet_segment(name).is_err(), "{name:?}");
        }
        // These are special only as the single path wallets/<name>, so a
        // wallet with either name still resolves beneath it.
        for name in ["new", "recover"] {
            assert_eq!(validate_wallet_segment(name), Ok(name));
        }
    }

    #[test]
    fn nothing_the_segment_validator_accepts_can_traverse() {
        // Every string of one or two bytes, then a deterministic sweep of
        // longer printable-ASCII strings built from a mixing step.
        let check = |name: &str| {
            if validate_wallet_segment(name).is_ok() {
                assert!(validate_wallet_id(name).is_ok(), "{name:?}");
                assert!(is_safe_segment(name), "{name:?}");
                assert!(
                    !name.contains(['/', '.', '\\', '\0']) && !name.is_empty(),
                    "{name:?}"
                );
                assert!(
                    name.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')),
                    "{name:?}"
                );
            }
        };
        for first in 0..=255u8 {
            if let Ok(one) = core::str::from_utf8(&[first]) {
                check(one);
            }
            for second in 0..=255u8 {
                if let Ok(two) = core::str::from_utf8(&[first, second]) {
                    check(two);
                }
            }
        }
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..200_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = 1 + (state % 70) as usize;
            let name: String = (0..len)
                .map(|offset| {
                    let mixed = state.rotate_left((offset * 7) as u32);
                    char::from(b' ' + (mixed % 95) as u8)
                })
                .collect();
            check(&name);
        }
    }

    #[test]
    fn the_wire_validator_stays_wider_than_the_segment_validator() {
        // Every segment-valid name is wire-valid.
        for name in ["a", "main", "alice-1", "trading_2", "a0", &"a".repeat(64)] {
            assert!(validate_wallet_segment(name).is_ok(), "{name:?}");
            assert!(validate_wallet_id(name).is_ok(), "{name:?}");
        }
        // `Token` admits `.` and `/`, though Bloom never creates such a name,
        // so only the segment validator may build a path segment or store key.
        for name in ["alice.vault", "team/alice"] {
            assert!(validate_wallet_id(name).is_ok(), "{name:?}");
            assert!(validate_wallet_segment(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn the_selected_account_comes_from_trusted_context() {
        let zero = wallet_account(&ctx("swaps/alice/0/new", "alice", 0)).unwrap();
        assert_eq!((zero.wallet(), zero.number()), ("alice", 0));
        let one = wallet_account(&ctx("swaps/alice/1/new", "alice", 1)).unwrap();
        assert_eq!((one.wallet(), one.number()), ("alice", 1));
        assert_eq!(one.to_string(), "alice/1");
    }

    #[test]
    fn missing_identity_is_refused_and_never_defaults_to_account_zero() {
        // The whole point of the type: a route Bloom did not mount on an
        // account gets an error, not account 0.
        let no_account = RouteCtx::new("swaps/alice/0/new")
            .param("bloom.wallet", "alice")
            .bind::<Swap>();
        assert_eq!(code(&wallet_account(&no_account).unwrap_err()), -2);

        let no_wallet = RouteCtx::new("swaps/alice/0/new")
            .param("bloom.account", "0")
            .bind::<Swap>();
        assert_eq!(code(&wallet_account(&no_wallet).unwrap_err()), -2);

        let bare = RouteCtx::new("swaps/alice/0/new").bind::<Swap>();
        assert_eq!(code(&wallet_account(&bare).unwrap_err()), -2);
    }

    #[test]
    fn the_trusted_wallet_must_match_the_route_capture() {
        // Bloom's router cannot produce this, so it is a tripwire rather than a
        // live path — but it is the check that makes the value trustworthy.
        let mismatched = ctx("swaps/mallory/0/new", "alice", 0);
        assert_eq!(code(&wallet_account(&mismatched).unwrap_err()), -2);
    }

    #[test]
    fn a_selected_wallet_must_be_one_safe_segment() {
        struct Status;
        impl RouteIdentity for Status {
            const PATH: &'static str = "status.json";
            const CANONICAL_PATH: &'static str = "status.json";
            const PARAMS: &'static [(&'static str, usize)] = &[];
        }
        // Bloom never selects either wallet; the check keeps one that did out
        // of every account path. `..` agrees with its own capture, so only the
        // segment check refuses it.
        let refused = route_invalid("selected wallet is not a single path segment");
        let dots = ctx("swaps/../0/new", "..", 0);
        assert_eq!(wallet_account(&dots).unwrap_err(), refused);
        // A capture is one segment, so a wallet containing `/` can only arrive
        // on a route with no `[wallet]` capture to compare it with.
        let nested = RouteCtx::new("status.json")
            .account("a/b", 0)
            .bind::<Status>();
        assert_eq!(wallet_account(&nested).unwrap_err(), refused);
    }

    #[test]
    fn a_caller_supplied_wallet_param_cannot_shadow_the_capture() {
        // Bloom never builds this context: its runner refuses caller params
        // that collide with trusted ones. If one did arrive, `route_param`
        // takes the first match, so the generated capture, which is
        // positional, is what the trusted wallet is checked against.
        let spoofed = RouteCtx::new("swaps/alice/0/new")
            .param("wallet", "mallory")
            .account("alice", 0)
            .bind::<Swap>();
        let account = wallet_account(&spoofed).expect("the capture still says alice");
        assert_eq!(account.wallet(), "alice");
    }

    #[test]
    fn an_unparseable_account_is_a_backend_fault() {
        for raw in ["", "x", "-1", "4294967296", " 1"] {
            let bad = RouteCtx::new("swaps/alice/0/new")
                .param("bloom.wallet", "alice")
                .param("bloom.account", raw)
                .bind::<Swap>();
            assert_eq!(code(&wallet_account(&bad).unwrap_err()), -4, "{raw:?}");
        }
    }

    #[test]
    fn an_account_path_cannot_leave_the_account_directory() {
        let account = testing::wallet_account("alice", 1);
        assert_eq!(account.dir(), "wallets/alice/1");
        assert_eq!(
            account.path("address.evm").unwrap(),
            "wallets/alice/1/address.evm"
        );
        assert_eq!(
            account.path("chains/base/balance.raw").unwrap(),
            "wallets/alice/1/chains/base/balance.raw"
        );
        // `".."` alone is caught by `is_safe_segment`; the rest are caught only
        // because every segment is checked separately.
        for rest in [
            "",
            "/",
            "/a",
            "a/",
            "a//b",
            "..",
            "../0/address.evm",
            "a/../../b",
            "a/./b",
            "a\\b",
            "a\0b",
        ] {
            assert_eq!(code(&account.path(rest).unwrap_err()), -3, "{rest:?}");
        }
    }

    #[test]
    fn two_accounts_never_share_a_path() {
        // One agent may hold several wallets. Nothing derived from the identity
        // may collide across them, or a Petal reads or writes the wrong one.
        let alice0 = testing::wallet_account("alice", 0);
        let alice1 = testing::wallet_account("alice", 1);
        let bob0 = testing::wallet_account("bob", 0);

        let paths = |a: &WalletAccount| {
            (
                a.dir(),
                a.address_evm_path(),
                a.address_sol_path(),
                a.path("swaps/current.json").unwrap(),
            )
        };
        assert_ne!(paths(&alice0), paths(&alice1));
        assert_ne!(paths(&alice0), paths(&bob0));
        assert_ne!(paths(&alice1), paths(&bob0));
        assert_eq!(alice1.address_evm_path(), "wallets/alice/1/address.evm");
        assert_eq!(bob0.address_sol_path(), "wallets/bob/0/address.sol");
    }

    #[test]
    fn an_evm_address_keeps_the_casing_bloom_projected() {
        // Bloom projects EIP-55. Lowercasing it would discard the checksum, so
        // a caller parsing the result into an address type could not check it.
        let checksummed = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
        let parsed = parse_evm_address(format!("{checksummed}\n").as_bytes()).unwrap();
        assert_eq!(parsed, checksummed);
        assert!(parse_evm_address(b"0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed").is_ok());

        for bad in [
            &b"not-an-address"[..],
            b"0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAe",
            b"0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAedd",
            b"5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            b"0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAeZ",
            b"",
            &[0xff, 0xfe],
        ] {
            assert!(parse_evm_address(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_solana_address_is_checked_for_shape_only() {
        let address = "7EYnhQoR9YM3N7UoaKRoA44Uy8JeaZV3qyouov87awMs";
        assert_eq!(
            parse_sol_address(format!("{address}\n").as_bytes()).unwrap(),
            address
        );
        for bad in [
            &b"0O1lI"[..],                                   // base58 excludes these
            b"7EYnhQoR9YM3N7UoaKRoA44Uy8JeaZV3qyouov87awM0", // '0' is not base58
            b"short",
            b"",
            &[0xff, 0xfe],
        ] {
            assert!(parse_sol_address(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn address_reads_map_host_failures_onto_dispatch_codes() {
        let account = testing::wallet_account("alice", 1);
        let path = account.address_evm_path();

        let mut present =
            MapVfs::new().file(&path, b"0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed\n");
        assert_eq!(
            account.address_evm_from(&mut present).unwrap(),
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
        );

        // A missing projected leaf (NotFound) and a leaf Bloom does not project
        // (NotAFile, which reaches the guest as Invalid) mean the same thing to
        // a caller: this account has no address of that family.
        let mut absent = MapVfs::new();
        assert_eq!(
            code(&account.address_evm_from(&mut absent).unwrap_err()),
            -1
        );
        let mut not_a_file = MapVfs::new().failing(&path, SdkError::Host(HostStatus::Invalid));
        assert_eq!(
            code(&account.address_evm_from(&mut not_a_file).unwrap_err()),
            -1
        );

        let mut denied = MapVfs::new().failing(&path, SdkError::Host(HostStatus::Denied));
        assert_eq!(
            code(&account.address_evm_from(&mut denied).unwrap_err()),
            -2
        );

        let mut corrupt = MapVfs::new().file(&path, b"not-an-address");
        let failure = account.address_evm_from(&mut corrupt).unwrap_err();
        assert_eq!(code(&failure), -4);
        match failure {
            DispatchResponse::Error { message, .. } => {
                assert!(message.contains(&path), "{message}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_account_reads_only_its_own_address() {
        // The fake serves account 0; account 1 must not find it.
        let zero = testing::wallet_account("alice", 0);
        let one = testing::wallet_account("alice", 1);
        let mut vfs = MapVfs::new().file(
            &zero.address_evm_path(),
            b"0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed\n",
        );
        assert!(zero.address_evm_from(&mut vfs).is_ok());
        assert_eq!(code(&one.address_evm_from(&mut vfs).unwrap_err()), -1);
    }
}

pub fn route_error(code: i32, message: String) -> RouteError {
    match code {
        -1 => RouteError::NotFound(message),
        -2 => RouteError::Denied(message),
        -3 => RouteError::Invalid(message),
        -4 => RouteError::Backend(message),
        _ => RouteError::Unsupported(message),
    }
}

pub fn read_json_value<T: serde::Serialize>(value: &T) -> DispatchResponse {
    match serde_json::to_vec_pretty(value) {
        Ok(bytes) => DispatchResponse::Read(bytes),
        Err(e) => error(-4, e.to_string()),
    }
}

pub fn read_store(key: &str, max_bytes: usize) -> DispatchResponse {
    match sdk::store_get(key, max_bytes) {
        Ok(bytes) => DispatchResponse::Read(bytes),
        Err(SdkError::Host(HostStatus::NotFound)) => error(-1, "not found"),
        Err(err) => error(-4, err.message()),
    }
}

/// Fixtures for testing route and domain code off `wasm32`.
///
/// Every Petal that tests a `Ctx`-taking function has hand-rolled these. The
/// local `impl RouteIdentity` stays with the test, since it documents the route
/// under test, but the `RawCtx` assembly and a fake VFS do not need repeating.
///
/// Production code must not reach for this module: [`wallet_account`] is the
/// only way a route should obtain a [`WalletAccount`].
pub mod testing {
    use super::{Ctx, RawCtx, RouteIdentity, SdkError, VfsRead, WalletAccount};
    use std::collections::BTreeMap;

    /// A well-formed package hash for tests: 64 lowercase hex digits, as Bloom
    /// supplies.
    pub const PACKAGE_HASH: &str =
        "aa00000000000000000000000000000000000000000000000000000000000001";
    /// The trusted route id [`RouteCtx`] supplies unless told otherwise.
    pub const ROUTE_ID: &str = "r000001";

    /// Assemble a [`Ctx`] for a route under test. It starts with a valid
    /// [`PACKAGE_HASH`] and the trusted `bloom.route_id` [`ROUTE_ID`], so a
    /// signing route can build its claim; change them with
    /// [`RouteCtx::package_hash`], [`RouteCtx::route_id`] and
    /// [`RouteCtx::without_route_id`].
    pub struct RouteCtx {
        path: String,
        params: Vec<(String, String)>,
        petal_root: String,
        package_hash: String,
        actor: Option<String>,
    }

    impl RouteCtx {
        /// `path` is the concrete request path, e.g. `swaps/alice/1/new`.
        pub fn new(path: &str) -> Self {
            Self {
                path: path.to_owned(),
                params: Vec::new(),
                petal_root: "test".to_owned(),
                package_hash: PACKAGE_HASH.to_owned(),
                actor: None,
            }
            .route_id(ROUTE_ID)
        }

        /// Set the trusted `bloom.route_id`, replacing any earlier one.
        pub fn route_id(self, route_id: &str) -> Self {
            let mut ctx = self.without_route_id();
            ctx.params
                .push(("bloom.route_id".to_owned(), route_id.to_owned()));
            ctx
        }

        /// Drop the trusted `bloom.route_id`, as on a context Bloom did not
        /// dispatch.
        pub fn without_route_id(mut self) -> Self {
            self.params.retain(|(name, _)| name != "bloom.route_id");
            self
        }

        /// Inject the trusted `bloom.wallet` and `bloom.account` pair Bloom
        /// supplies on an account-mounted route.
        pub fn account(mut self, wallet: &str, number: u32) -> Self {
            self.params
                .push(("bloom.wallet".to_owned(), wallet.to_owned()));
            self.params
                .push(("bloom.account".to_owned(), number.to_string()));
            self
        }

        /// Add one raw parameter, trusted or caller-supplied.
        pub fn param(mut self, name: &str, value: &str) -> Self {
            self.params.push((name.to_owned(), value.to_owned()));
            self
        }

        pub fn package_hash(mut self, package_hash: &str) -> Self {
            self.package_hash = package_hash.to_owned();
            self
        }

        pub fn actor(mut self, actor: &str) -> Self {
            self.actor = Some(actor.to_owned());
            self
        }

        pub fn raw(self) -> RawCtx {
            RawCtx {
                petal_root: self.petal_root,
                package_hash: self.package_hash,
                path: self.path,
                params: self.params,
                actor: self.actor,
            }
        }

        pub fn bind<I: RouteIdentity>(self) -> Ctx {
            Ctx::bind::<I>(self.raw())
        }
    }

    /// An in-memory [`VfsRead`] so address resolution can be tested where the
    /// real host imports are `unreachable!()`.
    #[derive(Default)]
    pub struct MapVfs {
        files: BTreeMap<String, Result<Vec<u8>, SdkError>>,
    }

    impl MapVfs {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn file(mut self, path: &str, bytes: &[u8]) -> Self {
            self.files.insert(path.to_owned(), Ok(bytes.to_vec()));
            self
        }

        /// Make a path fail with a specific host status.
        pub fn failing(mut self, path: &str, error: SdkError) -> Self {
            self.files.insert(path.to_owned(), Err(error));
            self
        }
    }

    impl VfsRead for MapVfs {
        fn read(&mut self, path: &str, _max_bytes: usize) -> Result<Vec<u8>, SdkError> {
            match self.files.get(path) {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(error)) => Err(error.clone()),
                None => Err(SdkError::Host(super::HostStatus::NotFound)),
            }
        }
    }

    /// A [`WalletAccount`] for domain code under test, bypassing [`Ctx`].
    pub fn wallet_account(wallet: &str, number: u32) -> WalletAccount {
        WalletAccount::for_test(wallet, number)
    }
}
