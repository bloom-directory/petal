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

pub fn payload_batch_digest(payloads: &[PayloadSignItem]) -> Result<[u8; 32], SdkError> {
    use sha2::{Digest as _, Sha256};

    if payloads.is_empty() {
        return Err(SdkError::Message(
            "payload signing batch must not be empty".into(),
        ));
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
pub enum PrivateInputKind {
    EvmAddress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateInputRequest {
    pub id: String,
    /// Passkey wallet authorizing release of the private value. When omitted,
    /// Bloom may select the sole passkey wallet on the machine.
    pub approval_wallet: Option<String>,
    /// Wallet whose Privacy Pools note is being withdrawn. This is context,
    /// not necessarily the passkey approval identity.
    pub wallet: String,
    pub title: String,
    pub prompt: String,
    pub kind: PrivateInputKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivateInputOutcome {
    Pending {
        ceremony_url: String,
        expires_ms: u64,
    },
    Ready(String),
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

pub mod sdk {
    pub use super::{
        DispatchResponse, EvmTransaction, HostStatus, HttpRequest, HttpResponse, OutboxApproval,
        OutboxInspection, PayloadBatchSignRequest, PayloadSignItem, PayloadSignRequest,
        PetalKeyOutcome, PetalKeyRequest, PrivateInputKind, PrivateInputOutcome,
        PrivateInputRequest, SdkError, SignBatchOutcome, SignOutcome, SignSelector,
        StagedTransaction, payload_batch_digest,
    };
    use crate::bindings::bloom::chain::read as chain;
    use crate::bindings::bloom::env::runtime as env;
    use crate::bindings::bloom::http::fetch as http;
    use crate::bindings::bloom::key::derive as key;
    use crate::bindings::bloom::private_input::ceremony as private_input;
    use crate::bindings::bloom::sign::signing as sign;
    use crate::bindings::bloom::store::kv as store;
    use crate::bindings::bloom::tx::outbox as tx;
    use crate::bindings::bloom::vfs::readwrite as vfs;

    const STATE_NS: &str = "state";
    const SECRET_NS: &str = "secrets";

    pub fn http_fetch(req: &HttpRequest, max_bytes: usize) -> Result<HttpResponse, SdkError> {
        let resp = http::fetch(&http::Request {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: req.headers.clone(),
            body: req.body.clone(),
        })
        .map_err(host_err)?;
        if resp.body.len() > max_bytes {
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
        let request: PetalKeyRequest = serde_json::from_slice(request_jcs)
            .map_err(|error| SdkError::Message(format!("decode Petal key request: {error}")))?;
        super::validate_wallet_id(&request.wallet_id).map_err(SdkError::Message)?;
        key::request(request_jcs).map_err(host_err)
    }

    pub fn derive_key(request: &PetalKeyRequest) -> Result<PetalKeyOutcome, SdkError> {
        super::validate_wallet_id(&request.wallet_id).map_err(SdkError::Message)?;
        let request_jcs = serde_jcs::to_vec(request)
            .map_err(|error| SdkError::Message(format!("encode Petal key request: {error}")))?;
        let outcome = request_key(&request_jcs)?;
        serde_json::from_slice(&outcome)
            .map_err(|error| SdkError::Message(format!("decode Petal key outcome: {error}")))
    }

    pub fn sign_payload(req: &PayloadSignRequest) -> Result<SignOutcome, SdkError> {
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
        super::validate_wallet_id(&req.wallet).map_err(SdkError::Message)?;
        let _ = payload_batch_digest(&req.payloads)?;
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

    pub fn request_private_input(
        request: &PrivateInputRequest,
    ) -> Result<PrivateInputOutcome, SdkError> {
        if let Some(wallet) = request.approval_wallet.as_deref() {
            super::validate_wallet_id(wallet).map_err(SdkError::Message)?;
        }
        let kind = match request.kind {
            PrivateInputKind::EvmAddress => private_input::InputKind::EvmAddress,
        };
        match private_input::request_input(&private_input::Request {
            id: request.id.clone(),
            wallet: request.wallet.clone(),
            approval_wallet: request.approval_wallet.clone(),
            title: request.title.clone(),
            prompt: request.prompt.clone(),
            kind,
        })
        .map_err(host_err)?
        {
            private_input::InputResult::Pending(pending) => Ok(PrivateInputOutcome::Pending {
                ceremony_url: pending.ceremony_url,
                expires_ms: pending.expires_ms,
            }),
            private_input::InputResult::Ready(value) => Ok(PrivateInputOutcome::Ready(value)),
        }
    }

    pub fn consume_private_input(id: &str) -> Result<(), SdkError> {
        private_input::consume(id).map_err(host_err)
    }

    pub fn chain_read(
        chain_name: &str,
        method: &str,
        params_json: &str,
    ) -> Result<String, SdkError> {
        chain::call(&chain::Request {
            chain: chain_name.into(),
            method: method.into(),
            params_json: params_json.into(),
        })
        .map(|response| response.result_json)
        .map_err(host_err)
    }

    pub fn store_get(key: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
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
        let namespace = namespace_for_key(key, secret);
        store::put(namespace, key, value, namespace == SECRET_NS).map_err(host_err)
    }

    pub fn store_put_new(key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        let namespace = namespace_for_key(key, secret);
        store::put_new(namespace, key, value, namespace == SECRET_NS).map_err(host_err)
    }

    pub fn store_del(key: &str) -> Result<(), SdkError> {
        let namespace = namespace_for_key(key, false);
        store::delete(namespace, key).map_err(host_err)
    }

    pub fn store_del_if_value(key: &str, expected: &[u8]) -> Result<(), SdkError> {
        let namespace = namespace_for_key(key, false);
        store::delete_if_value(namespace, key, expected).map_err(host_err)
    }

    pub fn store_list(prefix: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        let namespace = namespace_for_key(prefix, false);
        let keys = store::list(namespace, prefix).map_err(host_err)?;
        let size = keys.iter().map(|key| key.len()).sum::<usize>();
        if size > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall { needed: size }));
        }
        Ok(keys)
    }

    pub fn vfs_read(path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        let bytes = vfs::read(path).map_err(host_err)?;
        if bytes.len() > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall {
                needed: bytes.len(),
            }));
        }
        Ok(bytes)
    }

    pub fn vfs_write(path: &str, body: &[u8]) -> Result<(), SdkError> {
        vfs::write(path, body).map_err(host_err)
    }

    pub fn vfs_list(path: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        let _ = vfs::lookup(path).map_err(host_err)?;
        let entries = vfs::list(path).map_err(host_err)?;
        let size = entries.iter().map(|entry| entry.name.len()).sum::<usize>();
        if size > max_bytes {
            return Err(SdkError::Host(HostStatus::BufferTooSmall { needed: size }));
        }
        Ok(entries.into_iter().map(|entry| entry.name).collect())
    }

    pub fn now_ms() -> u64 {
        env::now_ms().unwrap_or(0)
    }

    pub fn try_now_ms() -> Result<u64, SdkError> {
        env::now_ms().map_err(host_err)
    }

    pub fn random_bytes(len: usize) -> Result<Vec<u8>, SdkError> {
        let len = u32::try_from(len).map_err(|_| SdkError::Host(HostStatus::Invalid))?;
        env::random_bytes(len).map_err(host_err)
    }

    pub fn runtime_setting(key: &str) -> Result<Option<String>, SdkError> {
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

    fn host_err(message: String) -> SdkError {
        let lower = message.to_ascii_lowercase();
        if lower.contains("not found") {
            SdkError::Host(HostStatus::NotFound)
        } else if lower.contains("denied") || lower.contains("permission") {
            SdkError::Host(HostStatus::Denied)
        } else if lower.contains("invalid") {
            SdkError::Host(HostStatus::Invalid)
        } else {
            SdkError::Message(message)
        }
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
